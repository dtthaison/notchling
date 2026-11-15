use crate::acting_ops::Room;
use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::{peers_of, Gate, NodeSource, Peer};
use notchling_engine::cluster::{qualify, NodeId};
use notchling_local_api::{topics, ApiError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const OUTBOX_LIST: &str = "outbox.list";
pub const REFRESH_MS: u64 = 30_000;
pub const MAX_ROWS: usize = 2_000;
const LOOP_EVERY: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    u64::try_from(crate::acting_ops::now_ms()).unwrap_or(0)
}

pub trait OutboxSource: Send + Sync {
    fn list(&self, node: &NodeId) -> Result<Value, ApiError>;
}

impl OutboxSource for NodeSource {
    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.ask(node, OUTBOX_LIST, json!({}))
    }
}

#[derive(Default)]
struct Remote {
    rows: BTreeMap<String, Value>,
    next_at: Option<u64>,
}

#[derive(Default)]
struct Book {
    nodes: BTreeMap<NodeId, Remote>,
    dirty: Vec<NodeId>,
}

pub struct RemoteOutbox {
    room: Arc<dyn Room>,
    gate: Gate,
    book: Mutex<Book>,
    nudged: Mutex<bool>,
    wake: Condvar,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn qualified_rows(node: &NodeId, answer: Value) -> Option<BTreeMap<String, Value>> {
    let Value::Array(rows) = answer else {
        return None;
    };
    let mut out = BTreeMap::new();
    for mut row in rows.into_iter().take(MAX_ROWS) {
        let (Some(id), Some(session)) = (text(&row, "id"), text(&row, "session")) else {
            continue;
        };
        let (id, session) = (id.to_string(), qualify(node, session));
        row["session"] = Value::String(session);
        out.insert(id, row);
    }
    Some(out)
}

fn removal(id: &str, session: &str, why: &str) -> Value {
    json!({"id": id, "session": session, "why": why})
}

impl RemoteOutbox {
    pub fn new(room: Arc<dyn Room>, gate: Gate) -> RemoteOutbox {
        RemoteOutbox {
            room,
            gate,
            book: Mutex::new(Book::default()),
            nudged: Mutex::new(false),
            wake: Condvar::new(),
        }
    }

    pub fn clear(&self) {
        let mut book = lock(&self.book);
        book.nodes.clear();
        book.dirty.clear();
    }

    pub fn nudge(&self) {
        *lock(&self.nudged) = true;
        self.wake.notify_all();
    }

    pub fn wait(&self, within: Duration) {
        let mut nudged = lock(&self.nudged);
        if !*nudged {
            nudged = self
                .wake
                .wait_timeout(nudged, within)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *nudged = false;
    }

    fn mark_dirty(&self, node: &NodeId) {
        {
            let mut book = lock(&self.book);
            if !book.dirty.contains(node) {
                book.dirty.push(node.clone());
            }
        }
        self.nudge();
    }

    fn listening(&self) -> bool {
        (self.gate)() && self.room.can_follow(topics::OUTBOX)
    }

    pub fn peer_event(&self, from: &NodeId, name: &str, data: &Value) {
        if !self.listening() {
            return;
        }
        if name == topics::OUTBOX_REMOVED {
            if let (Some(id), Some(session)) = (text(data, "id"), text(data, "session")) {
                if let Some(remote) = lock(&self.book).nodes.get_mut(from) {
                    remote.rows.remove(id);
                }
                let shown = qualify(from, session);
                let why = text(data, "why").unwrap_or("removed");
                self.room.publish(
                    topics::OUTBOX,
                    topics::OUTBOX_REMOVED,
                    &removal(id, &shown, why),
                );
            }
        }
        self.mark_dirty(from);
    }

    pub fn sync(&self, peers: &[Peer], source: &dyn OutboxSource, now: u64) {
        if !self.listening() {
            self.clear();
            return;
        }
        let wanted: Vec<NodeId> = {
            let mut book = lock(&self.book);
            book.nodes.retain(|id, _| peers.iter().any(|p| p.id == *id));
            let dirty = std::mem::take(&mut book.dirty);
            let mut wanted = Vec::new();
            for peer in peers {
                let remote = book.nodes.entry(peer.id.clone()).or_default();
                if !peer.online {
                    remote.next_at = None;
                    continue;
                }
                let due = remote.next_at.is_none_or(|at| now >= at);
                if due || dirty.contains(&peer.id) {
                    wanted.push(peer.id.clone());
                }
            }
            wanted
        };
        for id in wanted {
            let answer = source
                .list(&id)
                .ok()
                .and_then(|rows| qualified_rows(&id, rows));
            let events = {
                let mut book = lock(&self.book);
                let Some(remote) = book.nodes.get_mut(&id) else {
                    continue;
                };
                remote.next_at = Some(now + REFRESH_MS);
                match answer {
                    Some(fresh) => diff(&mut remote.rows, fresh),
                    None => Vec::new(),
                }
            };
            for (name, data) in events {
                self.room.publish(topics::OUTBOX, name, &data);
            }
        }
    }
}

fn diff(
    known: &mut BTreeMap<String, Value>,
    fresh: BTreeMap<String, Value>,
) -> Vec<(&'static str, Value)> {
    let mut events = Vec::new();
    for (id, row) in &fresh {
        if known.get(id) != Some(row) {
            events.push((topics::OUTBOX_CHANGED, json!({"row": row})));
        }
    }
    for (id, row) in known.iter() {
        if !fresh.contains_key(id) {
            let session = text(row, "session").unwrap_or_default();
            events.push((topics::OUTBOX_REMOVED, removal(id, session, "removed")));
        }
    }
    *known = fresh;
    events
}

pub struct OutboxFollower {
    stop: Arc<AtomicBool>,
    remote: Arc<RemoteOutbox>,
}

impl OutboxFollower {
    pub fn spawn(node: Arc<ClusterNode>, remote: Arc<RemoteOutbox>) -> OutboxFollower {
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, book) = (stop.clone(), remote.clone());
        let spawned = std::thread::Builder::new()
            .name("cluster-outbox".into())
            .spawn(move || {
                let source = NodeSource::new(node.clone());
                while !flag.load(Ordering::SeqCst) {
                    book.sync(&peers_of(&node.status()), &source, now_ms());
                    book.wait(LOOP_EVERY);
                }
            });
        if spawned.is_err() {
            stop.store(true, Ordering::SeqCst);
        }
        OutboxFollower { stop, remote }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.remote.nudge();
    }
}

#[cfg(test)]
#[path = "cluster_outbox_tests.rs"]
mod tests;
