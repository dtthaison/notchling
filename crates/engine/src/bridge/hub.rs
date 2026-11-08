//! The live clients, and what changed since each last heard: sessions, prompts and chat messages
//! (Swift `BridgeHub`). [`Hub::tick`] runs every 300 ms, which is also what debounces the pushes.

use super::backend::BridgeBackend;
use super::connection::{lock, Connection, ConnectionIo};
use super::model::Session;
use super::wire;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Hub {
    connections: Mutex<BTreeMap<u64, Arc<Connection>>>,
    next_id: AtomicU64,
}

impl Hub {
    pub fn new() -> Hub {
        Hub::default()
    }

    /// A new client. Nothing is served to it before `hello` or `pair`.
    pub fn connect(
        &self,
        remote_address: &str,
        is_tunnel: bool,
        io: Arc<dyn ConnectionIo>,
    ) -> Arc<Connection> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let connection = Arc::new(Connection::new(id, remote_address, is_tunnel, io));
        lock(&self.connections).insert(id, Arc::clone(&connection));
        connection
    }

    pub fn remove(&self, connection: &Connection) {
        lock(&self.connections).remove(&connection.id);
    }

    pub fn all(&self) -> Vec<Arc<Connection>> {
        lock(&self.connections).values().cloned().collect()
    }

    /// Closes every live connection of a device (after a revoke). Returns how many.
    pub fn disconnect(&self, device_id: &str) -> usize {
        let targets: Vec<_> = self
            .all()
            .into_iter()
            .filter(|c| c.device().is_some_and(|d| d.id == device_id))
            .collect();
        for connection in &targets {
            connection.close();
        }
        targets.len()
    }

    /// Tells every logged-in client to open. Returns how many were told.
    pub fn notify_open(&self) -> usize {
        let targets: Vec<_> = self
            .all()
            .into_iter()
            .filter(|c| c.is_authenticated())
            .collect();
        let message = wire::event("open", Map::new());
        for connection in &targets {
            connection.send_text(message.clone());
        }
        targets.len()
    }

    /// Pushes what changed to every subscriber.
    pub fn tick(&self, backend: &dyn BridgeBackend) {
        let subscribers: Vec<_> = self
            .all()
            .into_iter()
            .filter(|c| {
                c.is_authenticated() && {
                    let sub = c.subscription();
                    sub.wants_sessions || sub.chat_session_id.is_some()
                }
            })
            .collect();
        if subscribers.is_empty() {
            return;
        }

        if subscribers.iter().any(|c| c.wants_sessions()) {
            let sessions = backend.sessions();
            let json = sessions_json(&sessions);
            let prompts: Vec<_> = sessions.iter().filter_map(|s| s.pending.as_ref()).collect();
            let prompt_ids: HashSet<String> = prompts.iter().map(|p| p.id.clone()).collect();
            for connection in subscribers.iter().filter(|c| c.wants_sessions()) {
                let mut out = Vec::new();
                {
                    let mut sub = connection.subscription();
                    if sub.last_sessions_json.as_deref() != Some(json.as_str()) {
                        sub.last_sessions_json = Some(json.clone());
                        out.push(sessions_event(&sessions));
                    }
                    for prompt in &prompts {
                        if !sub.known_prompt_ids.contains(&prompt.id) {
                            let mut fields = Map::new();
                            fields.insert(
                                "sessionId".into(),
                                Value::from(prompt.session_id.as_str()),
                            );
                            fields.insert("prompt".into(), wire::object(&prompt.to_wire()));
                            out.push(wire::event("prompt", fields));
                        }
                    }
                    let mut resolved: Vec<_> = sub
                        .known_prompt_ids
                        .difference(&prompt_ids)
                        .cloned()
                        .collect();
                    resolved.sort();
                    for id in resolved {
                        let mut fields = Map::new();
                        fields.insert("requestId".into(), Value::from(id));
                        fields.insert("resolved".into(), Value::Bool(true));
                        out.push(wire::event("prompt", fields));
                    }
                    sub.known_prompt_ids = prompt_ids.clone();
                }
                out.into_iter().for_each(|text| connection.send_text(text));
            }
        }

        for connection in &subscribers {
            let (session_id, subagent_id) = {
                let sub = connection.subscription();
                match &sub.chat_session_id {
                    Some(id) => (id.clone(), sub.chat_subagent_id.clone()),
                    None => continue,
                }
            };
            let messages = backend
                .chat_messages(&session_id, subagent_id.as_deref())
                .unwrap_or_default();
            let fresh: Vec<_> = {
                let mut sub = connection.subscription();
                let fresh = messages
                    .iter()
                    .filter(|m| !sub.known_chat_ids.contains(&m.id))
                    .cloned()
                    .collect();
                sub.known_chat_ids = messages.iter().map(|m| m.id.clone()).collect();
                fresh
            };
            if !fresh.is_empty() {
                let mut fields = Map::new();
                fields.insert("sessionId".into(), Value::from(session_id));
                fields.insert("messages".into(), wire::object(&fresh));
                connection.send_text(wire::event("chat", fields));
            }
        }
    }
}

/// The text the hub compares to tell whether the list changed.
pub(crate) fn sessions_json(sessions: &[Session]) -> String {
    let list: Vec<_> = sessions.iter().map(Session::to_wire).collect();
    wire::text(&wire::object(&list))
}

/// The `sessions` event.
pub(crate) fn sessions_event(sessions: &[Session]) -> String {
    let list: Vec<_> = sessions.iter().map(Session::to_wire).collect();
    let mut fields = Map::new();
    fields.insert("data".into(), wire::object(&list));
    wire::event("sessions", fields)
}
