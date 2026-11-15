use crate::acting_ops::Room;
use crate::cluster_forward::{PromptPush, NODE_FOLLOW};
use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::{is_client, peers_of, NodeSource, Peer, RemoteSessions};
use notchling_engine::cluster::{qualify, NodeId};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

pub const FOLLOW_EVERY_MS: u64 = 30_000;
pub const LEASE_MS: u64 = crate::cluster_forward::FOLLOW_LEASE_MS;
pub const RETRY_MS: u64 = 5_000;
pub const MAX_PER_NODE: usize = 50;
pub const MAX_TOTAL: usize = 200;
pub const ID_MAX: usize = 128;
pub const TEXT_MAX: usize = 4_096;
pub const JSON_MAX: usize = 64 * 1024;
const LOOP_EVERY: Duration = Duration::from_secs(1);
const ITEM_FIELDS: [&str; 5] = ["header", "question", "options", "details", "multiSelect"];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    u64::try_from(crate::acting_ops::now_ms()).unwrap_or(0)
}

fn bare(value: Option<&Value>, allow_empty: bool) -> Option<String> {
    let text = value?.as_str()?;
    let fine = text.len() <= ID_MAX
        && !text.contains('/')
        && !text.chars().any(char::is_control)
        && (allow_empty || !text.is_empty());
    fine.then(|| text.to_string())
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(TEXT_MAX)
        .collect()
}

fn small(value: &Value) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= JSON_MAX)
}

fn clean_question(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let items = value.get("items")?.as_array()?;
    let mut kept = Vec::new();
    for item in items {
        let fields = item.as_object()?;
        let mut clean = Map::new();
        for key in ITEM_FIELDS {
            if let Some(v) = fields.get(key) {
                clean.insert(key.to_string(), v.clone());
            }
        }
        kept.push(Value::Object(clean));
    }
    let out = json!({"items": kept});
    small(&out).then_some(out)
}

pub fn clean_pending(
    node: &NodeId,
    raw: &Value,
    mirror: &dyn Fn(&NodeId, &str) -> Option<String>,
) -> Option<Value> {
    let fields = raw.as_object()?;
    let id = bare(fields.get("id"), false)?;
    let session = bare(fields.get("sessionId"), true)?;
    let kind = match fields.get("kind").and_then(Value::as_str)? {
        kind @ ("permission" | "question") => kind,
        _ => return None,
    };
    let question = clean_question(fields.get("question").unwrap_or(&Value::Null))?;
    let input = fields
        .get("input")
        .filter(|v| v.is_object() && small(v))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let owner_path = text(fields.get("transcriptPath"));
    let transcript = (!owner_path.is_empty())
        .then(|| mirror(node, &owner_path))
        .flatten();
    Some(json!({
        "id": qualify(node, &id),
        "sessionId": if session.is_empty() { String::new() } else { qualify(node, &session) },
        "kind": kind,
        "toolName": text(fields.get("toolName")),
        "summary": text(fields.get("summary")),
        "input": input,
        "canAlwaysAllow": false,
        "question": question,
        "cwd": text(fields.get("cwd")),
        "transcriptPath": transcript,
        "createdMs": fields.get("createdMs").and_then(Value::as_i64).unwrap_or(0),
        "node": node,
    }))
}

pub fn clean_resolved(node: &NodeId, raw: &Value) -> Option<(String, Value)> {
    let fields = raw.as_object()?;
    let id = bare(fields.get("id"), false)?;
    let session = bare(fields.get("sessionId"), true)?;
    let qualified = qualify(node, &id);
    let short = |key: &str| -> String { text(fields.get(key)).chars().take(32).collect() };
    let data = json!({
        "id": qualified,
        "sessionId": if session.is_empty() { String::new() } else { qualify(node, &session) },
        "outcome": short("outcome"),
        "by": short("by"),
        "node": node,
    });
    Some((qualified, data))
}

pub trait PromptSource: Send + Sync {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError>;
    fn list(&self, node: &NodeId) -> Result<Value, ApiError>;
}

impl PromptSource for NodeSource {
    fn follow(&self, node: &NodeId) -> Result<(), ApiError> {
        self.ask(node, NODE_FOLLOW, json!({"topic": topics::PROMPTS}))
            .map(|_| ())
    }

    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.ask(node, ops::PROMPTS_LIST, json!({}))
    }
}

#[derive(Default)]
struct Followed {
    next_at: u64,
    ok_at: Option<u64>,
    listed: bool,
    listing: bool,
    gone: BTreeSet<String>,
}

impl Followed {
    fn following(&self) -> bool {
        self.ok_at.is_some()
    }
}

#[derive(Default)]
struct Book {
    followed: BTreeMap<NodeId, Followed>,
    prompts: BTreeMap<String, (NodeId, Value)>,
}

impl Book {
    fn count(&self, node: &NodeId) -> usize {
        self.prompts.values().filter(|(n, _)| n == node).count()
    }

    fn put(&mut self, node: &NodeId, shaped: Value) -> bool {
        let Some(id) = shaped.get("id").and_then(Value::as_str).map(String::from) else {
            return false;
        };
        match self.prompts.get(&id) {
            Some((_, old)) if *old == shaped => return false,
            Some(_) => {}
            None if self.count(node) >= MAX_PER_NODE || self.prompts.len() >= MAX_TOTAL => {
                return false
            }
            None => {}
        }
        self.prompts.insert(id, (node.clone(), shaped));
        true
    }
}

pub struct RemotePrompts {
    room: Arc<dyn Room>,
    mirror: Arc<RemoteSessions>,
    book: Mutex<Book>,
}

fn resolved_for(shaped: &Value, by: &str) -> Value {
    json!({
        "id": shaped["id"], "sessionId": shaped["sessionId"],
        "outcome": "pass", "by": by, "node": shaped["node"],
    })
}

impl RemotePrompts {
    pub fn new(room: Arc<dyn Room>, mirror: Arc<RemoteSessions>) -> RemotePrompts {
        RemotePrompts {
            room,
            mirror,
            book: Mutex::new(Book::default()),
        }
    }

    fn announce(&self, event: &str, data: &Value) {
        self.room.publish(topics::PROMPTS, event, data);
    }

    fn mirrored(&self) -> impl Fn(&NodeId, &str) -> Option<String> + '_ {
        |node, path| self.mirror.mirror_of(node, path)
    }

    pub fn list(&self) -> Vec<Value> {
        let mut rows: Vec<Value> = lock(&self.book)
            .prompts
            .values()
            .map(|(_, p)| p.clone())
            .collect();
        rows.sort_by_key(|p| p.get("createdMs").and_then(Value::as_i64).unwrap_or(0));
        rows
    }

    pub fn merge(&self, local: Value, caller: &Caller) -> Value {
        if caller.client == ClientKind::Node {
            return local;
        }
        let mut rows = match local {
            Value::Array(rows) => rows,
            _ => Vec::new(),
        };
        rows.extend(self.list());
        Value::Array(rows)
    }

    pub fn drop_node(&self, node: &NodeId, why: &str) {
        let gone: Vec<Value> = {
            let mut book = lock(&self.book);
            book.followed.remove(node);
            let ids: Vec<String> = book
                .prompts
                .iter()
                .filter(|(_, (n, _))| n == node)
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter()
                .filter_map(|id| book.prompts.remove(id).map(|(_, p)| p))
                .collect()
        };
        for prompt in gone {
            self.announce(topics::PROMPT_RESOLVED, &resolved_for(&prompt, why));
        }
    }

    pub fn stop(&self, why: &str) {
        let nodes: Vec<NodeId> = {
            let book = lock(&self.book);
            book.followed
                .keys()
                .cloned()
                .chain(book.prompts.values().map(|(n, _)| n.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        for node in nodes {
            self.drop_node(&node, why);
        }
    }

    pub fn on_event(&self, from: &NodeId, event: &str, data: &Value) {
        let announce = match event {
            topics::PROMPT_PENDING => {
                let Some(shaped) = clean_pending(from, data, &self.mirrored()) else {
                    return;
                };
                let mut book = lock(&self.book);
                if !book.followed.get(from).is_some_and(Followed::following) {
                    return;
                }
                if let Some(id) = shaped.get("id").and_then(Value::as_str) {
                    if let Some(f) = book.followed.get_mut(from) {
                        f.gone.remove(id);
                    }
                }
                book.put(from, shaped.clone()).then_some((event, shaped))
            }
            topics::PROMPT_RESOLVED => {
                let Some((id, shaped)) = clean_resolved(from, data) else {
                    return;
                };
                let mut book = lock(&self.book);
                let Some(f) = book.followed.get_mut(from).filter(|f| f.following()) else {
                    return;
                };
                if f.listing {
                    f.gone.insert(id.clone());
                }
                let removed = matches!(book.prompts.get(&id), Some((n, _)) if n == from);
                if removed {
                    book.prompts.remove(&id);
                }
                removed.then_some((event, shaped))
            }
            _ => return,
        };
        if let Some((event, data)) = announce {
            self.announce(event, &data);
        }
    }

    fn fill(&self, node: &NodeId, listed: &Value) {
        let Value::Array(rows) = listed else {
            return;
        };
        let mut fresh = Vec::new();
        {
            let mut book = lock(&self.book);
            for row in rows.iter().filter(|r| r.get("tool").is_none()) {
                let Some(shaped) = clean_pending(node, row, &self.mirrored()) else {
                    continue;
                };
                let id = shaped.get("id").and_then(Value::as_str).unwrap_or_default();
                let skip = book
                    .followed
                    .get(node)
                    .is_none_or(|f| f.gone.contains(id) || !f.following());
                if !skip && book.put(node, shaped.clone()) {
                    fresh.push(shaped);
                }
            }
        }
        for prompt in fresh {
            self.announce(topics::PROMPT_PENDING, &prompt);
        }
    }

    fn renew(&self, node: &NodeId, source: &dyn PromptSource, now: u64) {
        match source.follow(node) {
            Ok(()) => {
                let listed = {
                    let mut book = lock(&self.book);
                    let f = book.followed.entry(node.clone()).or_default();
                    f.ok_at = Some(now);
                    f.next_at = now + FOLLOW_EVERY_MS;
                    f.listed
                };
                if !listed {
                    self.relist(node, source, now);
                }
            }
            Err(_) => {
                let lapsed = {
                    let mut book = lock(&self.book);
                    let f = book.followed.entry(node.clone()).or_default();
                    f.next_at = now + RETRY_MS;
                    f.ok_at.is_some_and(|at| now >= at + LEASE_MS)
                };
                if lapsed {
                    self.drop_node(node, "unfollowed");
                    lock(&self.book)
                        .followed
                        .entry(node.clone())
                        .or_default()
                        .next_at = now + RETRY_MS;
                }
            }
        }
    }

    fn relist(&self, node: &NodeId, source: &dyn PromptSource, now: u64) {
        if let Some(f) = lock(&self.book).followed.get_mut(node) {
            f.listing = true;
        }
        let answer = source.list(node);
        if let Ok(rows) = &answer {
            self.fill(node, rows);
        }
        let mut book = lock(&self.book);
        if let Some(f) = book.followed.get_mut(node) {
            f.listing = false;
            match answer {
                Ok(_) => {
                    f.listed = true;
                    f.gone.clear();
                }
                Err(_) => f.next_at = now + RETRY_MS,
            }
        }
    }

    pub fn sync(&self, peers: &[Peer], active: bool, source: &dyn PromptSource, now: u64) {
        if !active {
            self.stop("unfollowed");
            return;
        }
        let online: Vec<&Peer> = peers.iter().filter(|p| p.online).collect();
        let tracked: Vec<NodeId> = {
            let book = lock(&self.book);
            book.followed
                .keys()
                .cloned()
                .chain(book.prompts.values().map(|(n, _)| n.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        for node in tracked {
            if !online.iter().any(|p| p.id == node) {
                self.drop_node(&node, "offline");
            }
        }
        for peer in online {
            let due = lock(&self.book)
                .followed
                .get(&peer.id)
                .is_none_or(|f| now >= f.next_at);
            if due {
                self.renew(&peer.id, source, now);
            }
        }
    }
}

pub fn wants_follow(status: &Value, app_attached: bool) -> bool {
    app_attached && is_client(status)
}

pub struct PromptFollower {
    stop: Sender<()>,
}

impl PromptFollower {
    pub fn spawn(
        node: Arc<ClusterNode>,
        prompts: Arc<RemotePrompts>,
        room: Arc<dyn Room>,
    ) -> PromptFollower {
        let (stop, rx) = channel::<()>();
        let _ = std::thread::Builder::new()
            .name("cluster-prompts".into())
            .spawn(move || {
                let source = NodeSource::new(node.clone());
                loop {
                    let status = node.status();
                    let active = wants_follow(&status, room.can_answer());
                    prompts.sync(&peers_of(&status), active, &source, now_ms());
                    match rx.recv_timeout(LOOP_EVERY) {
                        Err(RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
                prompts.stop("unfollowed");
            });
        PromptFollower { stop }
    }

    pub fn close(&self) {
        let _ = self.stop.send(());
    }
}

#[derive(Default)]
pub struct NodePush {
    node: OnceLock<Weak<ClusterNode>>,
}

impl NodePush {
    pub fn attach(&self, node: &Arc<ClusterNode>) {
        let _ = self.node.set(Arc::downgrade(node));
    }
}

impl PromptPush for NodePush {
    fn push(&self, nodes: &[String], event: &str, data: &Value) {
        let Some(node) = self.node.get().and_then(Weak::upgrade) else {
            return;
        };
        let to: Vec<NodeId> = nodes.iter().filter_map(|n| NodeId::parse(n)).collect();
        node.tell_to(to, topics::PROMPTS, event, data.clone());
    }
}

#[cfg(test)]
#[path = "cluster_prompts_tests.rs"]
mod tests;
