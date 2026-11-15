use crate::acting_ops::Room;
use crate::cluster_forward::{FOLLOW_LEASE_MS, NODE_FOLLOW};
use crate::cluster_mirror::{is_real_file, mirror_path};
use crate::cluster_node::{ClusterNode, LocalCalls};
use crate::cluster_switches::LateHandler;
use notchling_engine::bridge::net::crypto::base64;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{qualify, LocalCmd, NodeId};
use notchling_engine::sessions::Agent;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const NODE_SESSIONS: &str = "node.sessions";
pub const NODE_TAIL: &str = "node.tail";
pub const NODE_OPS: [&str; 3] = [NODE_SESSIONS, NODE_TAIL, NODE_FOLLOW];
pub const TAIL_MAX: usize = 512 * 1024;
pub const STALE_MS: u64 = 30_000;
pub const RETRY_MS: u64 = 5_000;
pub const MAX_ROWS: usize = 500;
const LOOP_EVERY: Duration = Duration::from_secs(1);

fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    u64::try_from(crate::acting_ops::now_ms()).unwrap_or(0)
}

pub trait LocalRows: Send + Sync {
    fn rows(&self) -> Result<Vec<Value>, ApiError>;
}

pub struct HandlerRows {
    handler: Arc<LateHandler>,
}

impl HandlerRows {
    pub fn new(handler: Arc<LateHandler>) -> HandlerRows {
        HandlerRows { handler }
    }
}

impl LocalRows for HandlerRows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        let caller = Caller {
            tier: Tier::Agent,
            client: ClientKind::Node,
            peer: None,
            pid: None,
        };
        let list =
            self.handler
                .get()?
                .call(&caller, ops::SESSIONS_LIST, &json!({"tools": "all"}))?;
        match list {
            Value::Array(rows) => Ok(rows),
            _ => Err(failed("the session list isn't a list")),
        }
    }
}

#[cfg(unix)]
fn generation(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{}:{}", meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn generation(_meta: &std::fs::Metadata) -> String {
    String::new()
}

fn read_tail(path: &Path, from: u64, seen: Option<&str>) -> Result<Value, ApiError> {
    let mut file = std::fs::File::open(path).map_err(|e| failed(format!("the transcript: {e}")))?;
    let meta = file
        .metadata()
        .map_err(|e| failed(format!("the transcript: {e}")))?;
    let size = meta.len();
    let now = generation(&meta);
    let reset = from > size || seen.is_some_and(|g| g != now);
    let start = if reset { 0 } else { from };
    let want = (size - start).min(TAIL_MAX as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| failed(format!("the transcript: {e}")))?;
    let mut data = Vec::new();
    file.take(want)
        .read_to_end(&mut data)
        .map_err(|e| failed(format!("the transcript: {e}")))?;
    Ok(json!({
        "data": base64(&data),
        "next": start + data.len() as u64,
        "size": size,
        "reset": reset,
        "gen": now,
    }))
}

pub fn tail(rows: &[Value], args: &Value) -> Result<Value, ApiError> {
    let usage = "node.tail takes {\"id\": text, \"from\": number}";
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request(usage))?;
    let from = args.get("from").and_then(Value::as_u64).unwrap_or(0);
    let row = rows
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| failed("no such session on this node"))?;
    let path = row
        .get("transcriptPath")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| failed("that session has no transcript"))?;
    read_tail(
        Path::new(path),
        from,
        args.get("gen").and_then(Value::as_str),
    )
}

pub struct SessionCalls {
    inner: Arc<dyn LocalCalls>,
    rows: Arc<dyn LocalRows>,
    room: Arc<dyn Room>,
}

impl SessionCalls {
    pub fn new(
        inner: Arc<dyn LocalCalls>,
        rows: Arc<dyn LocalRows>,
        room: Arc<dyn Room>,
    ) -> SessionCalls {
        SessionCalls { inner, rows, room }
    }
}

impl LocalCalls for SessionCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_SESSIONS => Ok(Value::Array(self.rows.rows()?)),
            NODE_TAIL => tail(&self.rows.rows()?, args),
            NODE_FOLLOW => {
                if args.get("topic").and_then(Value::as_str) != Some(topics::PROMPTS) {
                    return Err(ApiError::bad_request(
                        "node.follow takes {\"topic\": \"prompts\"}",
                    ));
                }
                self.room
                    .follow_prompts(origin.node.as_str(), FOLLOW_LEASE_MS);
                Ok(json!({"following": true, "topic": topics::PROMPTS, "leaseMs": FOLLOW_LEASE_MS}))
            }
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub id: NodeId,
    pub name: String,
    pub online: bool,
}

pub fn peers_of(status: &Value) -> Vec<Peer> {
    let me = status.get("me").and_then(Value::as_str);
    let members = status.get("members").and_then(Value::as_array);
    members
        .into_iter()
        .flatten()
        .filter(|m| m.get("state").and_then(Value::as_str) == Some("active"))
        .filter_map(|m| {
            let id = NodeId::parse(m.get("id")?.as_str()?)?;
            (Some(id.as_str()) != me).then(|| Peer {
                id,
                name: m
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                online: m.get("online").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

pub trait RowSource: Send + Sync {
    fn rows(&self, node: &NodeId) -> Result<Value, ApiError>;
}

pub struct NodeSource {
    node: Arc<ClusterNode>,
}

impl NodeSource {
    pub fn new(node: Arc<ClusterNode>) -> NodeSource {
        NodeSource { node }
    }

    pub(crate) fn ask(&self, to: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let origin = Origin {
            node: self.node.me().clone(),
            tier: if op == NODE_SESSIONS {
                OriginTier::Node
            } else {
                OriginTier::App
            },
        };
        self.node.local(LocalCmd::Forward {
            node: to.clone(),
            op: op.to_string(),
            args,
            origin,
        })
    }
}

impl RowSource for NodeSource {
    fn rows(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.ask(node, NODE_SESSIONS, json!({}))
    }
}

#[derive(Default)]
struct Remote {
    name: String,
    online: bool,
    rows: Vec<Value>,
    next_at: Option<u64>,
}

#[derive(Default)]
struct Book {
    nodes: BTreeMap<NodeId, Remote>,
    dirty: Vec<NodeId>,
}

pub type Gate = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct RemoteSessions {
    mirror: PathBuf,
    gate: Gate,
    book: Mutex<Book>,
    nudged: Mutex<bool>,
    wake: Condvar,
}

fn valid(rows: Value) -> Option<Vec<Value>> {
    let Value::Array(rows) = rows else {
        return None;
    };
    Some(
        rows.into_iter()
            .filter_map(|row| {
                let tool = row.get("tool").and_then(Value::as_str).map(str::to_string);
                let agent: Agent = serde_json::from_value(row).ok()?;
                if agent.id.is_empty() {
                    return None;
                }
                let mut clean = serde_json::to_value(agent).ok()?;
                if let Some(tool) = tool {
                    clean["tool"] = Value::String(tool);
                }
                Some(clean)
            })
            .take(MAX_ROWS)
            .collect(),
    )
}

fn is_claude(row: &Value) -> bool {
    row.get("tool")
        .and_then(Value::as_str)
        .is_none_or(|tool| tool == "claude")
}

pub fn is_client(status: &Value) -> bool {
    let me = status.get("me").and_then(Value::as_str);
    status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|m| m.get("id").and_then(Value::as_str) == me)
        .and_then(|m| m.get("client").and_then(Value::as_bool))
        .unwrap_or(false)
}

impl RemoteSessions {
    pub fn new(mirror: PathBuf) -> RemoteSessions {
        RemoteSessions {
            mirror,
            gate: Arc::new(|| true),
            book: Mutex::new(Book::default()),
            nudged: Mutex::new(false),
            wake: Condvar::new(),
        }
    }

    pub fn with_gate(mut self, gate: Gate) -> RemoteSessions {
        self.gate = gate;
        self
    }

    pub fn allowed(&self) -> bool {
        (self.gate)()
    }

    pub fn clear(&self) {
        let mut book = lock(&self.book);
        book.nodes.clear();
        book.dirty.clear();
    }

    pub fn mark_dirty(&self, node: &NodeId) {
        {
            let mut book = lock(&self.book);
            if !book.dirty.contains(node) {
                book.dirty.push(node.clone());
            }
        }
        self.nudge();
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

    pub fn sync(&self, peers: &[Peer], source: &dyn RowSource, now: u64) {
        if !self.allowed() {
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
                remote.name = peer.name.clone();
                remote.online = peer.online;
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
            let answer = source.rows(&id).ok().and_then(valid);
            let mut book = lock(&self.book);
            let Some(remote) = book.nodes.get_mut(&id) else {
                continue;
            };
            match answer {
                Some(rows) => {
                    remote.rows = rows;
                    remote.next_at = Some(now + STALE_MS);
                }
                None => {
                    remote.next_at = Some(now + RETRY_MS);
                }
            }
        }
    }

    fn shaped(&self, node: &NodeId, remote: &Remote, row: &Value) -> Option<Value> {
        let fields = row.as_object()?;
        let id = fields.get("id")?.as_str()?;
        let mut out: Map<String, Value> = fields.clone();
        let qualified = qualify(node, id);
        out.insert("id".into(), json!(qualified));
        out.insert("host".into(), json!(node));
        out.insert(
            "node".into(),
            json!({"id": node, "name": remote.name, "online": remote.online}),
        );
        let mirrored = fields
            .get("transcriptPath")
            .and_then(Value::as_str)
            .and_then(|path| mirror_path(&self.mirror, node, &qualified, path))
            .filter(|path| is_real_file(path))
            .map(|path| json!(path.to_string_lossy()));
        out.insert("transcriptPath".into(), mirrored.unwrap_or(Value::Null));
        Some(Value::Object(out))
    }

    pub fn rows(&self) -> Vec<Value> {
        self.rows_for(true)
    }

    fn rows_for(&self, all: bool) -> Vec<Value> {
        if !self.allowed() {
            return Vec::new();
        }
        let book = lock(&self.book);
        book.nodes
            .iter()
            .flat_map(|(node, remote)| {
                remote
                    .rows
                    .iter()
                    .filter(move |row| all || is_claude(row))
                    .filter_map(|row| self.shaped(node, remote, row))
            })
            .collect()
    }

    pub fn merge(&self, local: &Value) -> Value {
        Self::joined(local, self.rows_for(false))
    }

    pub fn merge_all(&self, local: &Value) -> Value {
        Self::joined(local, self.rows_for(true))
    }

    fn joined(local: &Value, remote: Vec<Value>) -> Value {
        let mut rows = match local {
            Value::Array(rows) => rows.clone(),
            _ => Vec::new(),
        };
        rows.extend(remote);
        Value::Array(rows)
    }

    pub fn raw(&self, node: &NodeId, id: &str) -> Option<Value> {
        let book = lock(&self.book);
        book.nodes
            .get(node)?
            .rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .cloned()
    }
}

pub struct Refresher {
    stop: Arc<AtomicBool>,
    remote: Arc<RemoteSessions>,
}

impl Refresher {
    pub fn spawn(
        node: Arc<ClusterNode>,
        remote: Arc<RemoteSessions>,
        publish: Arc<dyn Fn() + Send + Sync>,
    ) -> Refresher {
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, book) = (stop.clone(), remote.clone());
        let spawned = std::thread::Builder::new()
            .name("cluster-sessions".into())
            .spawn(move || {
                let source = NodeSource::new(node.clone());
                let mut last: Vec<Value> = Vec::new();
                while !flag.load(Ordering::SeqCst) {
                    book.sync(&peers_of(&node.status()), &source, now_ms());
                    let rows = book.rows();
                    if rows != last {
                        last = rows;
                        publish();
                    }
                    book.wait(LOOP_EVERY);
                }
            });
        if spawned.is_err() {
            stop.store(true, Ordering::SeqCst);
        }
        Refresher { stop, remote }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.remote.nudge();
    }
}

#[cfg(test)]
pub(crate) fn agent_row(id: &str) -> Value {
    json!({
        "id": id, "name": id, "project": "p", "cwd": "/tmp", "group": "completed",
        "label": "Idle", "sinceMs": 1, "target": {"job": id}, "host": "Background",
        "headless": false,
    })
}

#[cfg(test)]
#[path = "cluster_sessions_tests.rs"]
mod tests;
