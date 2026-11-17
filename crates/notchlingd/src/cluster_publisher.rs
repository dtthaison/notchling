use crate::acting_ops::Room;
use crate::cluster_sessions::RemoteSessions;
use notchling_local_api::topics;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct Publisher {
    room: Arc<dyn Room>,
    remote: Arc<RemoteSessions>,
    local: Arc<Mutex<Value>>,
    last: Mutex<Option<Value>>,
    last_shell: Mutex<Option<Value>>,
    last_codex: Mutex<Option<Value>>,
}

impl Publisher {
    pub fn new(
        room: Arc<dyn Room>,
        remote: Arc<RemoteSessions>,
        local: Arc<Mutex<Value>>,
    ) -> Publisher {
        Publisher {
            room,
            remote,
            local,
            last: Mutex::new(None),
            last_shell: Mutex::new(None),
            last_codex: Mutex::new(None),
        }
    }

    fn publish_apart(&self, last: &Mutex<Option<Value>>, rows: Vec<Value>, event: &str) {
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        let rows = Value::Array(rows);
        let changed = match last.as_ref() {
            Some(seen) => *seen != rows,
            None => rows.as_array().is_some_and(|rows| !rows.is_empty()),
        };
        if !changed {
            return;
        }
        self.room.publish(topics::SESSIONS, event, &rows);
        *last = Some(rows);
    }

    pub fn publish(&self) {
        self.publish_apart(
            &self.last_shell,
            self.remote.shell_rows(),
            topics::SESSIONS_REMOTE_SHELL,
        );
        self.publish_apart(
            &self.last_codex,
            self.remote.codex_rows(),
            topics::SESSIONS_REMOTE_CODEX,
        );
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let local = self.local.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let merged = self.remote.merge(&local);
        if last.as_ref() == Some(&merged) {
            return;
        }
        self.room
            .publish(topics::SESSIONS, topics::SESSIONS_CHANGED, &merged);
        *last = Some(merged);
    }
}
