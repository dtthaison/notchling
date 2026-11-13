use super::process::{Killer, Output, Spawned, Spawner, ToolProcess};
use serde_json::Value;
use std::io;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

pub type Responder = Arc<dyn Fn(&Value, &FakeHandle) + Send + Sync>;

#[derive(Clone)]
pub struct FakeHandle {
    inner: Arc<FakeInner>,
}

struct FakeInner {
    written: Mutex<Vec<String>>,
    tx: Mutex<Sender<Output>>,
    killed: Mutex<bool>,
    responder: Option<Responder>,
}

impl FakeHandle {
    pub fn push_line(&self, line: &str) {
        let _ = self.send(Output::Line(line.to_string()));
    }

    pub fn push(&self, message: &Value) {
        self.push_line(&message.to_string());
    }

    pub fn push_oversize(&self) {
        let _ = self.send(Output::Oversize);
    }

    pub fn die(&self, message: Option<&str>) {
        let _ = self.send(Output::Closed(message.map(str::to_string)));
    }

    pub fn break_pipe(&self) {
        *self.inner.killed.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    pub fn written(&self) -> Vec<Value> {
        self.inner
            .written
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn requests(&self, method: &str) -> Vec<Value> {
        self.written()
            .into_iter()
            .filter(|m| m.get("method").and_then(Value::as_str) == Some(method))
            .collect()
    }

    pub fn killed(&self) -> bool {
        *self.inner.killed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, out: Output) -> bool {
        self.inner
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send(out)
            .is_ok()
    }
}

struct FakeProcess {
    handle: FakeHandle,
}

impl ToolProcess for FakeProcess {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        if self.handle.killed() {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "killed"));
        }
        self.handle
            .inner
            .written
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(line.to_string());
        if let (Some(responder), Ok(message)) = (
            &self.handle.inner.responder,
            serde_json::from_str::<Value>(line),
        ) {
            responder(&message, &self.handle);
        }
        Ok(())
    }

    fn killer(&self) -> Arc<dyn Killer> {
        Arc::new(FakeKiller {
            handle: self.handle.clone(),
        })
    }
}

struct FakeKiller {
    handle: FakeHandle,
}

impl Killer for FakeKiller {
    fn kill(&self) {
        let mut killed = self
            .handle
            .inner
            .killed
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !*killed {
            *killed = true;
            drop(killed);
            self.handle.die(None);
        }
    }
}

#[derive(Default)]
pub struct FakeSpawner {
    make: Option<Arc<dyn Fn() -> Responder + Send + Sync>>,
    spawned: Mutex<Vec<FakeHandle>>,
    calls: Mutex<Vec<(String, Vec<String>, String)>>,
    refuse: Mutex<Option<String>>,
}

impl FakeSpawner {
    pub fn new() -> FakeSpawner {
        FakeSpawner::default()
    }

    pub fn with_responder(responder: Responder) -> FakeSpawner {
        FakeSpawner {
            make: Some(Arc::new(move || responder.clone())),
            ..FakeSpawner::default()
        }
    }

    pub fn replaying(recording: &str) -> FakeSpawner {
        let recording = recording.to_string();
        FakeSpawner {
            make: Some(Arc::new(move || replaying(&recording))),
            ..FakeSpawner::default()
        }
    }

    pub fn refuse_with(&self, message: &str) {
        *self.refuse.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.to_string());
    }

    pub fn spawned(&self) -> Vec<FakeHandle> {
        self.spawned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn calls(&self) -> Vec<(String, Vec<String>, String)> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Spawner for FakeSpawner {
    fn spawn(&self, program: &str, args: &[String], folder: &str) -> Result<Spawned, String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).push((
            program.to_string(),
            args.to_vec(),
            folder.to_string(),
        ));
        if let Some(message) = self
            .refuse
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(message);
        }
        let (tx, rx) = channel();
        let handle = FakeHandle {
            inner: Arc::new(FakeInner {
                written: Mutex::new(Vec::new()),
                tx: Mutex::new(tx),
                killed: Mutex::new(false),
                responder: self.make.as_ref().map(|make| make()),
            }),
        };
        self.spawned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handle.clone());
        Ok(Spawned {
            process: Box::new(FakeProcess { handle }),
            output: rx,
        })
    }
}

pub fn replaying(recording: &str) -> Responder {
    struct Cursor {
        lines: Vec<Value>,
        next: usize,
        answering: Option<Value>,
        awaiting: Option<Value>,
    }
    let lines: Vec<Value> = recording
        .lines()
        .filter_map(|l| l.strip_prefix("< "))
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let cursor = Arc::new(Mutex::new(Cursor {
        lines,
        next: 0,
        answering: None,
        awaiting: None,
    }));
    Arc::new(move |message: &Value, process: &FakeHandle| {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        let mut cursor = cursor.lock().unwrap_or_else(|e| e.into_inner());
        match (method, id) {
            (Some("initialize" | "session/load" | "session/prompt"), Some(id)) => {
                cursor.answering = Some(id);
            }
            (None, Some(id)) if cursor.awaiting.as_ref() == Some(&id) => cursor.awaiting = None,
            _ => return,
        }
        while cursor.next < cursor.lines.len() {
            let mut line = cursor.lines[cursor.next].clone();
            cursor.next += 1;
            let is_response = line.get("method").is_none();
            if is_response {
                if let Some(id) = cursor.answering.clone() {
                    line["id"] = id;
                }
            }
            process.push(&line);
            if line.get("method").is_some() && line.get("id").is_some() {
                cursor.awaiting = line.get("id").cloned();
                break;
            }
            if is_response {
                break;
            }
        }
    })
}
