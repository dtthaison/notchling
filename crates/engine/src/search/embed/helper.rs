use super::{EmbedError, Embedder, ModelId};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const CACHE: usize = 256;
pub const QUERIES_AT_ONCE: usize = 2;
pub const MAX_TEXTS: usize = 8;
pub const READY_WAIT: Duration = Duration::from_secs(60);
pub const QUERY_WAIT: Duration = Duration::from_secs(10);
pub const BATCH_WAIT: Duration = Duration::from_secs(60);
pub const RETRY_AFTER: Duration = Duration::from_secs(30);
const PUMP_WAIT: Duration = Duration::from_millis(200);
const DEFAULT_DIMS: usize = 384;

pub trait HelperIo: Send + Sync {
    fn send(&self, line: &str) -> Result<(), String>;
    fn recv(&self, wait: Duration) -> Result<Option<String>, String>;
}

pub trait HelperSpawn: Send + Sync {
    fn spawn(&self) -> Result<Box<dyn HelperIo>, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperState {
    Cold,
    Warming,
    Ready,
    Failed(String),
}

enum Reply {
    Vecs(Vec<Vec<f32>>),
    Error(String),
}

struct Conn {
    io: Arc<dyn HelperIo>,
    waiting: Mutex<HashMap<u64, Sender<Reply>>>,
    alive: AtomicBool,
    next: AtomicU64,
}

enum CallError {
    Helper(String),
    Dead(String),
}

fn unpack(text: &str, dims: usize) -> Option<Vec<f32>> {
    let bytes = STANDARD.decode(text).ok()?;
    if bytes.len() != dims * 4 {
        return None;
    }
    Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
    )
}

impl Conn {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Sender<Reply>>> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn dispatch(&self, line: &str, dims: usize) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let Some(id) = value.get("id").and_then(Value::as_u64) else {
            return;
        };
        let Some(tx) = self.lock().remove(&id) else {
            return;
        };
        let reply = if let Some(e) = value.get("error").and_then(Value::as_str) {
            Reply::Error(e.to_string())
        } else {
            let vecs: Option<Vec<Vec<f32>>> = value
                .get("vecs")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().map(|v| unpack(v.as_str()?, dims)).collect());
            match vecs {
                Some(v) => Reply::Vecs(v),
                None => Reply::Error("the helper sent vectors in a shape not known".to_string()),
            }
        };
        let _ = tx.send(reply);
    }

    fn pump(self: Arc<Self>, dims: usize, inner: Arc<Inner>) {
        loop {
            match self.io.recv(PUMP_WAIT) {
                Ok(Some(line)) => self.dispatch(&line, dims),
                Ok(None) => {
                    if !self.alive.load(Ordering::SeqCst) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        self.alive.store(false, Ordering::SeqCst);
        self.lock().clear();
        inner.lose(&self);
    }

    fn call(&self, lane: &str, texts: &[&str], wait: Duration) -> Result<Vec<Vec<f32>>, CallError> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(CallError::Dead("the helper is gone".to_string()));
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        self.lock().insert(id, tx);
        let line = json!({"id": id, "lane": lane, "texts": texts}).to_string();
        if let Err(e) = self.io.send(&line) {
            self.lock().remove(&id);
            self.alive.store(false, Ordering::SeqCst);
            return Err(CallError::Dead(e));
        }
        match rx.recv_timeout(wait) {
            Ok(Reply::Vecs(v)) if v.len() == texts.len() => Ok(v),
            Ok(Reply::Vecs(_)) => Err(CallError::Helper(
                "the helper answered a different number of texts".to_string(),
            )),
            Ok(Reply::Error(e)) => Err(CallError::Helper(e)),
            Err(RecvTimeoutError::Timeout) => {
                self.lock().remove(&id);
                Err(CallError::Helper(
                    "the helper did not answer in time".to_string(),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(CallError::Dead("the helper stopped".to_string()))
            }
        }
    }

    fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Cache {
    order: VecDeque<String>,
    vectors: HashMap<String, Vec<f32>>,
}

impl Cache {
    fn put(&mut self, text: &str, vector: Vec<f32>) {
        if self.vectors.insert(text.to_string(), vector).is_none() {
            self.order.push_back(text.to_string());
        }
        while self.order.len() > CACHE {
            if let Some(old) = self.order.pop_front() {
                self.vectors.remove(&old);
            }
        }
    }
}

struct Slots {
    free: Mutex<usize>,
    open: Condvar,
}

struct Inner {
    state: Mutex<HelperState>,
    generation: AtomicU64,
    failed_at: Mutex<Option<Instant>>,
    conn: Mutex<Option<Arc<Conn>>>,
    dims: AtomicUsize,
    cache: Mutex<Cache>,
    slots: Slots,
}

impl Inner {
    fn lose(&self, dead: &Arc<Conn>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut slot = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, dead)) {
            *slot = None;
            if *state == HelperState::Ready {
                *state = HelperState::Cold;
            }
        }
        drop(slot);
        drop(state);
        dead.close();
    }
}

pub struct HelperEmbedder {
    spawn: Arc<dyn HelperSpawn>,
    model: ModelId,
    inner: Arc<Inner>,
}

fn ready_dims(line: &str, model: &ModelId) -> Result<usize, String> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("the helper's first line: {e}"))?;
    if value.get("ready").and_then(Value::as_bool) != Some(true) {
        return Err("the helper did not say it was ready".to_string());
    }
    if value.get("model").and_then(Value::as_str) != Some(model.0.as_str()) {
        return Err("the helper runs another model".to_string());
    }
    value
        .get("dims")
        .and_then(Value::as_u64)
        .filter(|d| *d > 0 && *d <= 4096)
        .map(|d| d as usize)
        .ok_or_else(|| "the helper sent no dimensions".to_string())
}

fn handshake(
    io: &dyn HelperIo,
    model: &ModelId,
    stopped: &dyn Fn() -> bool,
) -> Result<usize, String> {
    let started = Instant::now();
    loop {
        if stopped() {
            return Err("the helper was stopped".to_string());
        }
        let left = READY_WAIT.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err("the helper did not start in time".to_string());
        }
        if let Some(line) = io.recv(left.min(PUMP_WAIT))? {
            return ready_dims(&line, model);
        }
    }
}

impl HelperEmbedder {
    pub fn new(spawn: Arc<dyn HelperSpawn>, model: ModelId) -> HelperEmbedder {
        HelperEmbedder {
            spawn,
            model,
            inner: Arc::new(Inner {
                state: Mutex::new(HelperState::Cold),
                generation: AtomicU64::new(0),
                failed_at: Mutex::new(None),
                conn: Mutex::new(None),
                dims: AtomicUsize::new(DEFAULT_DIMS),
                cache: Mutex::new(Cache::default()),
                slots: Slots {
                    free: Mutex::new(QUERIES_AT_ONCE),
                    open: Condvar::new(),
                },
            }),
        }
    }

    pub fn state(&self) -> HelperState {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn warm(&self) {
        let generation = {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            match &*state {
                HelperState::Warming | HelperState::Ready => return,
                HelperState::Failed(_) => {
                    let failed = self
                        .inner
                        .failed_at
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if failed.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
                        return;
                    }
                }
                HelperState::Cold => {}
            }
            *state = HelperState::Warming;
            self.inner.generation.load(Ordering::SeqCst)
        };
        let inner = self.inner.clone();
        let spawn = self.spawn.clone();
        let model = self.model.clone();
        std::thread::spawn(move || {
            let stopped = || inner.generation.load(Ordering::SeqCst) != generation;
            let made = spawn
                .spawn()
                .and_then(|io| handshake(io.as_ref(), &model, &stopped).map(|dims| (io, dims)));
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if stopped() {
                return;
            }
            match made {
                Ok((io, dims)) => {
                    let conn = Arc::new(Conn {
                        io: Arc::from(io),
                        waiting: Mutex::new(HashMap::new()),
                        alive: AtomicBool::new(true),
                        next: AtomicU64::new(0),
                    });
                    inner.dims.store(dims, Ordering::SeqCst);
                    *inner.conn.lock().unwrap_or_else(|e| e.into_inner()) = Some(conn.clone());
                    let pumped = conn.clone();
                    let held = inner.clone();
                    std::thread::spawn(move || pumped.pump(dims, held));
                    *state = HelperState::Ready;
                }
                Err(e) => {
                    *inner.failed_at.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(Instant::now());
                    *state = HelperState::Failed(e);
                }
            }
        });
    }

    pub fn stop(&self) {
        let conn = {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            self.inner.generation.fetch_add(1, Ordering::SeqCst);
            *state = HelperState::Cold;
            self.inner
                .conn
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        };
        if let Some(conn) = conn {
            conn.close();
        }
    }

    fn connection(&self) -> Option<Arc<Conn>> {
        let conn = self
            .inner
            .conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        if conn.alive.load(Ordering::SeqCst) {
            Some(conn)
        } else {
            self.lose(&conn);
            None
        }
    }

    fn lose(&self, dead: &Arc<Conn>) {
        self.inner.lose(dead);
    }

    fn call(
        &self,
        lane: &str,
        texts: &[&str],
        wait: Duration,
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let Some(conn) = self.connection() else {
            if let HelperState::Failed(e) = self.state() {
                self.warm();
                return Err(EmbedError::Failed(e));
            }
            self.warm();
            return Err(EmbedError::NotReady);
        };
        match conn.call(lane, texts, wait) {
            Ok(v) => Ok(v),
            Err(CallError::Helper(e)) => Err(EmbedError::Failed(e)),
            Err(CallError::Dead(e)) => {
                self.lose(&conn);
                Err(EmbedError::Failed(e))
            }
        }
    }

    fn take_slot(&self) {
        let slots = &self.inner.slots;
        let mut free = slots.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = slots
                .open
                .wait_timeout(free, Duration::from_millis(250))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *free -= 1;
    }

    fn give_slot(&self) {
        let slots = &self.inner.slots;
        *slots.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        slots.open.notify_one();
    }

    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if let Some(v) = self
            .inner
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .vectors
            .get(text)
        {
            return Ok(v.clone());
        }
        self.take_slot();
        let result = self.call("query", &[text], QUERY_WAIT);
        self.give_slot();
        let mut vectors = result?;
        let vector = vectors.pop().ok_or(EmbedError::NotReady)?;
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(text, vector.clone());
        Ok(vector)
    }

    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut out = Vec::with_capacity(texts.len());
        for part in texts.chunks(MAX_TEXTS) {
            out.extend(self.call("index", part, BATCH_WAIT)?);
        }
        Ok(out)
    }
}

impl Embedder for HelperEmbedder {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dims(&self) -> usize {
        self.inner.dims.load(Ordering::SeqCst)
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.embed_batch(texts)
    }
}

pub struct ProcessSpawn {
    pub exe: PathBuf,
    pub model_dir: PathBuf,
}

struct ProcessIo {
    stdin: Mutex<std::process::ChildStdin>,
    lines: Mutex<std::sync::mpsc::Receiver<String>>,
    child: Mutex<std::process::Child>,
}

impl HelperIo for ProcessIo {
    fn send(&self, line: &str) -> Result<(), String> {
        use std::io::Write;
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .map_err(|e| e.to_string())
    }

    fn recv(&self, wait: Duration) -> Result<Option<String>, String> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        match lines.recv_timeout(wait) {
            Ok(line) => Ok(Some(line)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("the helper exited".to_string()),
        }
    }
}

impl Drop for ProcessIo {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl HelperSpawn for ProcessSpawn {
    fn spawn(&self) -> Result<Box<dyn HelperIo>, String> {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&self.exe)
            .arg("serve")
            .arg("--model")
            .arg(&self.model_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", self.exe.display()))?;
        let stdin = child.stdin.take().ok_or("the helper has no input")?;
        let stdout = child.stdout.take().ok_or("the helper has no output")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Box::new(ProcessIo {
            stdin: Mutex::new(stdin),
            lines: Mutex::new(rx),
            child: Mutex::new(child),
        }))
    }
}
