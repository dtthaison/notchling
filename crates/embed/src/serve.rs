use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const IDLE_EXIT: Duration = Duration::from_secs(300);
pub const MAX_TEXTS: usize = 8;
const POLL: Duration = Duration::from_millis(20);

impl Encode for crate::granite::Granite {
    fn model(&self) -> &str {
        crate::granite::MODEL
    }

    fn dims(&self) -> usize {
        crate::granite::Granite::dims(self)
    }

    fn encode(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        self.embed(texts)
    }
}

pub fn load(dir: &std::path::Path) -> Result<crate::granite::Granite, String> {
    crate::bench::limit_threads(1);
    crate::granite::Granite::load(dir)
}

pub trait Encode: Send + Sync {
    fn model(&self) -> &str;
    fn dims(&self) -> usize;
    fn encode(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Query,
    Index,
}

struct Request {
    id: u64,
    texts: Vec<String>,
}

pub(crate) type Sink = Arc<Mutex<Box<dyn Write + Send>>>;

fn write_line(sink: &Sink, value: &Value) -> bool {
    let mut out = sink.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(out, "{value}").is_ok() && out.flush().is_ok()
}

fn pack(vector: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for x in vector {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

fn background(lane: Lane) {
    #[cfg(target_os = "macos")]
    {
        let class = match lane {
            Lane::Query => libc::qos_class_t::QOS_CLASS_USER_INITIATED,
            Lane::Index => libc::qos_class_t::QOS_CLASS_BACKGROUND,
        };
        unsafe {
            libc::pthread_set_qos_class_self_np(class, 0);
        }
    }
    #[cfg(target_os = "linux")]
    if lane == Lane::Index {
        unsafe {
            libc::nice(10);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = lane;
}

pub(crate) struct Shared {
    sink: Sink,
    pub(crate) pending: AtomicUsize,
    activity: AtomicUsize,
    pub(crate) eof: AtomicBool,
}

impl Shared {
    pub(crate) fn new(sink: Sink) -> Shared {
        Shared {
            sink,
            pending: AtomicUsize::new(0),
            activity: AtomicUsize::new(0),
            eof: AtomicBool::new(false),
        }
    }

    pub(crate) fn drained(&self) -> bool {
        let eof = self.eof.load(Ordering::SeqCst);
        eof && self.pending.load(Ordering::SeqCst) == 0
    }
}

fn work(lane: Lane, model: Arc<dyn Encode>, rx: Receiver<Request>, shared: Arc<Shared>) {
    background(lane);
    while let Ok(request) = rx.recv() {
        let texts: Vec<&str> = request.texts.iter().map(String::as_str).collect();
        let reply = match model.encode(&texts) {
            Ok(vecs) if vecs.len() == texts.len() => {
                let packed: Vec<String> = vecs.iter().map(|v| pack(v)).collect();
                json!({"id": request.id, "vecs": packed})
            }
            Ok(_) => {
                json!({"id": request.id, "error": "the model answered the wrong number of texts"})
            }
            Err(e) => json!({"id": request.id, "error": e}),
        };
        write_line(&shared.sink, &reply);
        shared.activity.fetch_add(1, Ordering::SeqCst);
        shared.pending.fetch_sub(1, Ordering::SeqCst);
    }
}

fn parse(line: &str) -> Result<(Lane, Request), (u64, String)> {
    let value: Value = serde_json::from_str(line).map_err(|e| (0, format!("bad line: {e}")))?;
    let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
    let lane = match value.get("lane").and_then(Value::as_str) {
        Some("query") => Lane::Query,
        Some("index") => Lane::Index,
        _ => return Err((id, "lane is query or index".to_string())),
    };
    let texts: Vec<String> = match value.get("texts").and_then(Value::as_array) {
        Some(items) => items
            .iter()
            .map(|t| t.as_str().map(str::to_string))
            .collect::<Option<_>>()
            .ok_or((id, "texts are strings".to_string()))?,
        None => return Err((id, "texts is a list".to_string())),
    };
    if texts.is_empty() || texts.len() > MAX_TEXTS {
        return Err((id, format!("1 to {MAX_TEXTS} texts")));
    }
    Ok((lane, Request { id, texts }))
}

fn read(
    mut input: impl BufRead,
    query: Sender<Request>,
    index: Sender<Request>,
    shared: Arc<Shared>,
) {
    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        shared.activity.fetch_add(1, Ordering::SeqCst);
        match parse(text) {
            Ok((lane, request)) => {
                shared.pending.fetch_add(1, Ordering::SeqCst);
                let to = if lane == Lane::Query { &query } else { &index };
                if to.send(request).is_err() {
                    shared.pending.fetch_sub(1, Ordering::SeqCst);
                }
            }
            Err((id, why)) => {
                write_line(&shared.sink, &json!({"id": id, "error": why}));
            }
        }
    }
    shared.eof.store(true, Ordering::SeqCst);
}

pub fn serve(
    model: Arc<dyn Encode>,
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
    clock: &dyn Fn() -> Instant,
) -> Result<(), String> {
    let sink: Sink = Arc::new(Mutex::new(Box::new(output)));
    let ready = json!({"ready": true, "model": model.model(), "dims": model.dims()});
    if !write_line(&sink, &ready) {
        return Err("the output is closed".to_string());
    }
    let shared = Arc::new(Shared::new(sink));
    let (query_tx, query_rx) = channel();
    let (index_tx, index_rx) = channel();
    for (lane, rx) in [(Lane::Query, query_rx), (Lane::Index, index_rx)] {
        let (model, shared) = (model.clone(), shared.clone());
        std::thread::spawn(move || work(lane, model, rx, shared));
    }
    let reader = shared.clone();
    std::thread::spawn(move || read(input, query_tx, index_tx, reader));
    let mut seen = shared.activity.load(Ordering::SeqCst);
    let mut last = clock();
    loop {
        std::thread::sleep(POLL);
        if shared.drained() {
            return Ok(());
        }
        let busy = shared.pending.load(Ordering::SeqCst) > 0;
        let now = shared.activity.load(Ordering::SeqCst);
        if busy || now != seen {
            seen = now;
            last = clock();
        } else if clock().saturating_duration_since(last) >= IDLE_EXIT {
            return Ok(());
        }
    }
}
