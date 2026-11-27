use super::helper::{HelperIo, HelperSpawn};
use super::ModelId;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FAKE_DIMS: usize = 4;

struct Counters {
    spawns: AtomicUsize,
    requests: AtomicUsize,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
    delay_ms: AtomicU64,
    die_after: AtomicUsize,
    refuse: AtomicBool,
    live: AtomicUsize,
    exits: Mutex<Vec<Arc<AtomicBool>>>,
    gate: Mutex<Option<Receiver<()>>>,
    hook: Mutex<Option<RequestHook>>,
}

pub type RequestHook = Arc<dyn Fn(usize) + Send + Sync>;

#[derive(Clone)]
pub struct FakeSpawn {
    model: ModelId,
    counters: Arc<Counters>,
}

impl FakeSpawn {
    pub fn new(model: &ModelId) -> FakeSpawn {
        FakeSpawn {
            model: model.clone(),
            counters: Arc::new(Counters {
                spawns: AtomicUsize::new(0),
                requests: AtomicUsize::new(0),
                in_flight: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                delay_ms: AtomicU64::new(0),
                die_after: AtomicUsize::new(usize::MAX),
                refuse: AtomicBool::new(false),
                live: AtomicUsize::new(0),
                exits: Mutex::new(Vec::new()),
                gate: Mutex::new(None),
                hook: Mutex::new(None),
            }),
        }
    }

    pub fn delay(&self, by: Duration) {
        self.counters
            .delay_ms
            .store(by.as_millis() as u64, Ordering::SeqCst);
    }

    pub fn die_after(&self, requests: usize) {
        self.counters.die_after.store(requests, Ordering::SeqCst);
    }

    pub fn refuse(&self, on: bool) {
        self.counters.refuse.store(on, Ordering::SeqCst);
    }

    pub fn exit_all(&self) {
        for dead in self
            .counters
            .exits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            dead.store(true, Ordering::SeqCst);
        }
    }

    pub fn hold_spawn(&self) -> Sender<()> {
        let (release, wait) = channel();
        *self.counters.gate.lock().unwrap_or_else(|e| e.into_inner()) = Some(wait);
        release
    }

    pub fn on_request(&self, hook: RequestHook) {
        *self.counters.hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    pub fn live(&self) -> usize {
        self.counters.live.load(Ordering::SeqCst)
    }

    pub fn spawns(&self) -> usize {
        self.counters.spawns.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.counters.requests.load(Ordering::SeqCst)
    }

    pub fn peak_in_flight(&self) -> usize {
        self.counters.peak.load(Ordering::SeqCst)
    }

    pub fn vector_of(text: &str) -> Vec<f32> {
        vec![text.chars().count() as f32, 1.0, 0.0, 0.0]
    }
}

struct FakeIo {
    counters: Arc<Counters>,
    out: Sender<String>,
    lines: Mutex<Receiver<String>>,
    dead: Arc<AtomicBool>,
    die_after: usize,
    served: AtomicUsize,
}

fn pack(vector: &[f32]) -> String {
    let mut bytes = Vec::new();
    for x in vector {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

impl Drop for FakeIo {
    fn drop(&mut self) {
        self.counters.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl HelperIo for FakeIo {
    fn send(&self, line: &str) -> Result<(), String> {
        if self.dead.load(Ordering::SeqCst) {
            return Err("the helper exited".to_string());
        }
        let n = self.counters.requests.fetch_add(1, Ordering::SeqCst) + 1;
        let hook = self
            .counters
            .hook
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(n);
        }
        if self.served.fetch_add(1, Ordering::SeqCst) >= self.die_after {
            self.dead.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let id = value["id"].as_u64().unwrap_or(0);
        let texts: Vec<String> = value["texts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let now = self.counters.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.counters.peak.fetch_max(now, Ordering::SeqCst);
        let (counters, out) = (self.counters.clone(), self.out.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(
                counters.delay_ms.load(Ordering::SeqCst),
            ));
            let vecs: Vec<String> = texts
                .iter()
                .map(|t| pack(&FakeSpawn::vector_of(t)))
                .collect();
            counters.in_flight.fetch_sub(1, Ordering::SeqCst);
            let _ = out.send(json!({"id": id, "vecs": vecs}).to_string());
        });
        Ok(())
    }

    fn recv(&self, wait: Duration) -> Result<Option<String>, String> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        match lines.recv_timeout(wait) {
            Ok(line) => Ok(Some(line)),
            Err(RecvTimeoutError::Timeout) if self.dead.load(Ordering::SeqCst) => {
                Err("the helper exited".to_string())
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("the helper exited".to_string()),
        }
    }
}

impl HelperSpawn for FakeSpawn {
    fn spawn(&self) -> Result<Box<dyn HelperIo>, String> {
        let gate = self
            .counters
            .gate
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(gate) = gate {
            let _ = gate.recv_timeout(Duration::from_secs(20));
        }
        if self.counters.refuse.load(Ordering::SeqCst) {
            self.counters.spawns.fetch_add(1, Ordering::SeqCst);
            return Err("no helper here".to_string());
        }
        let (out, lines) = channel();
        let _ =
            out.send(json!({"ready": true, "model": self.model.0, "dims": FAKE_DIMS}).to_string());
        let dead = Arc::new(AtomicBool::new(false));
        self.counters
            .exits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(dead.clone());
        self.counters.live.fetch_add(1, Ordering::SeqCst);
        self.counters.spawns.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeIo {
            counters: self.counters.clone(),
            out,
            lines: Mutex::new(lines),
            dead,
            die_after: self.counters.die_after.load(Ordering::SeqCst),
            served: AtomicUsize::new(0),
        }))
    }
}
