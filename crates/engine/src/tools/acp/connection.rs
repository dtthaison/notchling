//! One Agent Client Protocol connection: requests with callbacks, notifications and the agent's own
//! requests handed to a [`Handler`]. Everything the process prints is handled on one thread, in
//! order, so a handler sees updates in the order they arrived.
//!
//! Callbacks (`Done`, the handler) run on that thread, or on the caller's thread when the
//! connection is already closed. They must not be called with a lock held that they take themselves.

use super::process::{Killer, Output, Spawned, ToolProcess};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

/// How long a message waits for another thread's write to finish before the process is given up
/// on. A write only takes this long when the agent isn't reading its input (it hangs, or isn't
/// listening), and under the local API's call timeout so the op that asked still answers.
const WRITER_WAIT: Duration = Duration::from_secs(3);

/// Runs with a request's result (`result` object, `{}` when it had none) or its error text.
pub type Done = Box<dyn FnOnce(Result<Value, String>) + Send>;

/// Who hears what the agent sends on its own.
pub trait Handler: Send + Sync {
    /// A message with a method and no id (`session/update`).
    fn notification(&self, method: &str, params: &Value);
    /// A call that wants an answer: `id` as it came (a string or a number), to be given back to
    /// [`Connection::respond`] or [`Connection::respond_error`] unchanged.
    fn request(&self, id: &Value, method: &str, params: &Value);
    /// The process ended on its own (not through [`Connection::close`]), with its last stderr line.
    fn exit(&self, message: Option<String>);
}

#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

struct Inner {
    /// The tool's name, for error messages.
    name: String,
    state: Mutex<State>,
    /// Held for the length of a write, which blocks while the agent isn't reading.
    process: Mutex<Box<dyn ToolProcess>>,
    /// Ends the process without the writer's lock, so a blocked write fails instead of holding
    /// up `close`.
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
    /// Wraps a started process; nothing is read until [`Connection::start`].
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

    /// Starts reading, handing what the agent sends to `handler`. Once.
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
                    // Dropped by the reader: nothing to act on, and the connection carries on.
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

    /// A request; `done` runs with the result or the error.
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

    /// Answers the agent's request `id`.
    pub fn respond(&self, id: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    pub fn respond_error(&self, id: &Value, code: i64, message: &str) {
        self.send(
            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        );
    }

    /// Writes a message; a broken pipe ends the connection, failing whatever waited.
    fn send(&self, message: &Value) {
        if !self.inner.write(message) {
            self.inner.ended(None);
        }
    }

    /// Ends the process; whatever waited fails. The handler's `exit` doesn't run.
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

    /// Writes one message; false when the connection is closed or the pipe broke.
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

    /// The writer's lock, waiting a little for a write in progress; `None` when it stays busy
    /// ([`WRITER_WAIT`]) or the connection closes meanwhile.
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

    /// The process ended (or its pipe broke): everything waiting fails with its last stderr line,
    /// and the handler hears of it unless we closed it ourselves.
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

/// A response's id: an integer, or a string holding one.
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
