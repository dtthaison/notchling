use super::process::{Killer, Output, Spawned, ToolProcess};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

const WRITER_WAIT: Duration = Duration::from_secs(3);

pub type Done = Box<dyn FnOnce(Result<Value, String>) + Send>;

pub trait Handler: Send + Sync {
    fn notification(&self, method: &str, params: &Value);
    fn request(&self, id: &Value, method: &str, params: &Value);
    fn exit(&self, message: Option<String>);
}

#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

struct Inner {
    name: String,
    state: Mutex<State>,
    process: Mutex<Box<dyn ToolProcess>>,
    killer: Arc<dyn Killer>,
    output: Mutex<Option<Receiver<Output>>>,
    handler: OnceLock<Arc<dyn Handler>>,
}

struct State {
    next_id: i64,
    waiting: HashMap<i64, Done>,
    closed: bool,
}

impl Connection {
    pub fn new(spawned: Spawned, name: &str) -> Connection {
        let killer = spawned.process.killer();
        Connection {
            inner: Arc::new(Inner {
                name: name.to_string(),
                state: Mutex::new(State {
                    next_id: 1,
                    waiting: HashMap::new(),
                    closed: false,
                }),
                process: Mutex::new(spawned.process),
                killer,
                output: Mutex::new(Some(spawned.output)),
                handler: OnceLock::new(),
            }),
        }
    }

    pub fn start(&self, handler: Arc<dyn Handler>) {
        let Some(rx) = self
            .inner
            .output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let _ = self.inner.handler.set(handler);
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            for out in rx {
                match out {
                    Output::Line(line) => inner.handle(&line),
                    Output::Oversize => {}
                    Output::Closed(message) => {
                        inner.ended(message);
                        return;
                    }
                }
            }
            inner.ended(None);
        });
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock_state().closed
    }

    pub fn request(&self, method: &str, params: Value, done: Done) {
        let id = {
            let mut state = self.inner.lock_state();
            if state.closed {
                drop(state);
                done(Err(format!("{} isn't running.", self.inner.name)));
                return;
            }
            let id = state.next_id;
            state.next_id += 1;
            state.waiting.insert(id, done);
            id
        };
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn respond(&self, id: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    pub fn respond_error(&self, id: &Value, code: i64, message: &str) {
        self.send(
            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        );
    }

    fn send(&self, message: &Value) {
        if !self.inner.write(message) {
            self.inner.ended(None);
        }
    }

    pub fn close(&self) {
        let waiting = {
            let mut state = self.inner.lock_state();
            if state.closed {
                return;
            }
            state.closed = true;
            std::mem::take(&mut state.waiting)
        };
        self.inner.kill();
        self.inner
            .fail(waiting, &format!("{} was closed.", self.inner.name));
    }
}

impl Inner {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, message: &Value) -> bool {
        if self.lock_state().closed {
            return false;
        }
        let Ok(line) = serde_json::to_string(message) else {
            return false;
        };
        let Some(mut process) = self.writer() else {
            return false;
        };
        process.write_line(&line).is_ok()
    }

    fn writer(&self) -> Option<MutexGuard<'_, Box<dyn ToolProcess>>> {
        let until = Instant::now() + WRITER_WAIT;
        loop {
            match self.process.try_lock() {
                Ok(guard) => return Some(guard),
                Err(TryLockError::Poisoned(e)) => return Some(e.into_inner()),
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= until || self.lock_state().closed {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    fn kill(&self) {
        self.killer.kill();
    }

    fn handle(&self, line: &str) {
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = match message.get("params") {
                Some(p @ Value::Object(_)) => p.clone(),
                _ => Value::Object(Map::new()),
            };
            let Some(handler) = self.handler.get() else {
                return;
            };
            match message.get("id") {
                Some(id) if !id.is_null() => handler.request(id, method, &params),
                _ => handler.notification(method, &params),
            }
            return;
        }
        let Some(id) = message.get("id").and_then(response_id) else {
            return;
        };
        let Some(done) = self.lock_state().waiting.remove(&id) else {
            return;
        };
        if let Some(error) = message.get("error").filter(|e| e.is_object()) {
            let text = error.get("message").and_then(Value::as_str).map_or_else(
                || format!("{} returned an error.", self.name),
                str::to_string,
            );
            done(Err(text));
        } else {
            let result = match message.get("result") {
                Some(r @ Value::Object(_)) => r.clone(),
                _ => Value::Object(Map::new()),
            };
            done(Ok(result));
        }
    }

    fn ended(&self, message: Option<String>) {
        let (waiting, was_closed) = {
            let mut state = self.lock_state();
            let was = state.closed;
            state.closed = true;
            (std::mem::take(&mut state.waiting), was)
        };
        self.kill();
        let text = message
            .clone()
            .unwrap_or_else(|| format!("{} stopped.", self.name));
        self.fail(waiting, &text);
        if !was_closed {
            if let Some(handler) = self.handler.get() {
                handler.exit(message);
            }
        }
    }

    fn fail(&self, waiting: HashMap<i64, Done>, message: &str) {
        let mut ordered: Vec<(i64, Done)> = waiting.into_iter().collect();
        ordered.sort_by_key(|(id, _)| *id);
        for (_, done) in ordered {
            done(Err(message.to_string()));
        }
    }
}

fn response_id(id: &Value) -> Option<i64> {
    match id {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
