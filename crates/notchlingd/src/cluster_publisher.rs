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
        }
    }

    pub fn publish(&self) {
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
