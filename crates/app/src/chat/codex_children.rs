use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notchling_engine::tools::codex::children::{ChildSource, DayFolders, Head};
use notchling_local_api::{Client, ClientError, ClientKind, ConnectOptions};
use serde_json::{json, Value};

use crate::actions::{call_timeout, Transport};

pub const OP: &str = "codex.children";
pub const SKIP_AFTER_FAILURE: Duration = Duration::from_secs(30);

pub struct DaemonChildren {
    transport: Box<dyn Transport>,
    failed: Mutex<Option<Instant>>,
}

impl DaemonChildren {
    pub fn new(transport: Box<dyn Transport>) -> DaemonChildren {
        DaemonChildren {
            transport,
            failed: Mutex::new(None),
        }
    }

    fn skipping(&self) -> bool {
        self.failed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| at.elapsed() < SKIP_AFTER_FAILURE)
    }

    fn note(&self, failed: bool) {
        *self.failed.lock().unwrap_or_else(|e| e.into_inner()) = failed.then(Instant::now);
    }
}

pub fn heads(reply: &Value) -> Vec<Head> {
    reply["children"]
        .as_array()
        .map(|rows| rows.iter().filter_map(head).collect())
        .unwrap_or_default()
}

fn head(row: &Value) -> Option<Head> {
    let text = |key: &str| row[key].as_str().map(str::to_owned);
    Some(Head {
        path: PathBuf::from(row["path"].as_str()?),
        thread: text("thread").filter(|t| !t.is_empty())?,
        parent: text("parent"),
        depth: u8::try_from(row["depth"].as_u64().unwrap_or(1)).unwrap_or(u8::MAX),
        nickname: text("nickname"),
        role: text("role"),
        started: row["started"].as_f64().unwrap_or(0.0),
    })
}

pub trait Line: Send {
    fn call(&mut self, op: &str, args: Value) -> Result<Value, ClientError>;
}

impl Line for Client {
    fn call(&mut self, op: &str, args: Value) -> Result<Value, ClientError> {
        Client::call(self, op, args)
    }
}

pub type Open = Box<dyn Fn() -> Result<Box<dyn Line>, ClientError> + Send + Sync>;

pub struct Kept {
    open: Open,
    line: Mutex<Option<Box<dyn Line>>>,
}

impl Kept {
    pub fn new(open: Open) -> Kept {
        Kept {
            open,
            line: Mutex::new(None),
        }
    }

    pub fn socket(socket: PathBuf, version: String) -> Kept {
        Kept::new(Box::new(move || {
            let options = ConnectOptions {
                timeout: call_timeout(OP),
                ..ConnectOptions::default()
            };
            Client::connect_with(&socket, ClientKind::App, &version, &options)
                .map(|client| Box::new(client) as Box<dyn Line>)
        }))
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn Line>>> {
        self.line.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Transport for Kept {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let taken = self.slot().take();
        let mut line = match taken {
            Some(line) => line,
            None => (self.open)()?,
        };
        let reply = line.call(op, args);
        if reply.is_ok() {
            *self.slot() = Some(line);
        }
        reply
    }

    fn close(&self) {
        self.slot().take();
    }
}

impl ChildSource for DaemonChildren {
    fn done(&self) {
        self.transport.close();
    }

    fn children(&self, parent_thread: &str, parent_path: &Path) -> Vec<Head> {
        if !self.skipping() {
            match self.transport.call(OP, json!({ "thread": parent_thread })) {
                Ok(reply) => {
                    self.note(false);
                    return heads(&reply);
                }
                Err(_) => self.note(true),
            }
        }
        DayFolders.children(parent_thread, parent_path)
    }
}

#[cfg(test)]
#[path = "codex_children_tests.rs"]
mod tests;
