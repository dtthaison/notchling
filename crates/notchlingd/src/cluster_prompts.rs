use crate::acting_ops::Room;
use crate::cluster_forward::{PromptPush, NODE_FOLLOW};
use crate::cluster_node::ClusterNode;
use crate::cluster_prompts_clean::{clean_pending, clean_resolved, Found};
use crate::cluster_sessions::{is_client, peers_of, NodeSource, Peer, RemoteSessions};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

pub use crate::cluster_prompts_clean::{ID_MAX, INPUT_MAX, QUESTION_MAX, TEXT_MAX};

pub const FOLLOW_EVERY_MS: u64 = 30_000;
pub const LEASE_MS: u64 = crate::cluster_forward::FOLLOW_LEASE_MS;
pub const RETRY_MS: u64 = 5_000;
pub const MAX_PER_NODE: usize = 50;
pub const MAX_TOTAL: usize = 200;
const LOOP_EVERY: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    u64::try_from(crate::acting_ops::now_ms()).unwrap_or(0)
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

pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;

fn threaded() -> Spawn {
    Arc::new(|job: Job| {
        let _ = std::thread::Builder::new()
            .name("cluster-prompts-ask".into())
            .spawn(job);
    })
}

#[derive(Default)]
struct Followed {
    next_at: u64,
    ok_at: Option<u64>,
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
    busy: BTreeSet<NodeId>,
    epoch: u64,
}

impl Book {
    fn count(&self, node: &NodeId) -> usize {
        self.prompts.values().filter(|(n, _)| n == node).count()
    }

    fn ids_of(&self, node: &NodeId) -> BTreeSet<String> {
        self.prompts
            .iter()
            .filter(|(_, (n, _))| n == node)
            .map(|(id, _)| id.clone())
            .collect()
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
    spawn: Spawn,
}

fn resolved_for(shaped: &Value, by: &str) -> Value {
    let mut data = json!({
        "id": shaped["id"], "sessionId": shaped["sessionId"],
        "outcome": "pass", "by": by, "node": shaped["node"],
    });
    if let Some(row) = shaped.get("rowId") {
        data["rowId"] = row.clone();
    }
    data
}

impl RemotePrompts {
    pub fn new(room: Arc<dyn Room>, mirror: Arc<RemoteSessions>) -> Arc<RemotePrompts> {
        RemotePrompts::with_spawn(room, mirror, threaded())
    }

    pub fn with_spawn(
        room: Arc<dyn Room>,
        mirror: Arc<RemoteSessions>,
        spawn: Spawn,
    ) -> Arc<RemotePrompts> {
        Arc::new(RemotePrompts {
            room,
            mirror,
            book: Mutex::new(Book::default()),
            spawn,
        })
    }

    fn announce(&self, event: &str, data: &Value) {
        self.room.publish(topics::PROMPTS, event, data);
    }

    fn find(&self) -> impl Fn(&NodeId, &str, &str) -> Found + '_ {
        |node, path, session| Found {
            mirror: (!path.is_empty())
                .then(|| self.mirror.mirror_of(node, path))
                .flatten(),
            row: self.mirror.row_of(node, path, session),
        }
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
        Value::Array(crate::prompts::fit_list(rows))
    }

    pub fn drop_node(&self, node: &NodeId, why: &str) {
        let gone: Vec<Value> = {
            let mut book = lock(&self.book);
            book.epoch += 1;
            book.followed.remove(node);
            let ids = book.ids_of(node);
            ids.iter()
                .filter_map(|id| book.prompts.remove(id).map(|(_, p)| p))
                .collect()
        };
        for prompt in gone {
            self.announce(topics::PROMPT_RESOLVED, &resolved_for(&prompt, why));
        }
    }

    pub fn stop(&self, why: &str) {
        let nodes: BTreeSet<NodeId> = {
            let book = lock(&self.book);
            book.followed
                .keys()
                .cloned()
                .chain(book.prompts.values().map(|(n, _)| n.clone()))
                .collect()
        };
        for node in nodes {
            self.drop_node(&node, why);
        }
    }

    pub fn on_event(&self, from: &NodeId, event: &str, data: &Value) {
        let announce = match event {
            topics::PROMPT_PENDING => {
                let Some(shaped) = clean_pending(from, data, &self.find()) else {
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
                let Some((id, mut shaped)) = clean_resolved(from, data) else {
                    return;
                };
                let mut book = lock(&self.book);
                let Some(f) = book.followed.get_mut(from).filter(|f| f.following()) else {
                    return;
                };
                if f.listing {
                    f.gone.insert(id.clone());
                }
                let held = match book.prompts.get(&id) {
                    Some((n, p)) if n == from => Some(p.clone()),
                    _ => None,
                };
                if let Some(prompt) = &held {
                    book.prompts.remove(&id);
                    if let Some(row) = prompt.get("rowId") {
                        shaped["rowId"] = row.clone();
                    }
                }
                held.map(|_| (event, shaped))
            }
            _ => return,
        };
        if let Some((event, data)) = announce {
            self.announce(event, &data);
        }
    }

    fn fill(&self, node: &NodeId, listed: &Value, before: &BTreeSet<String>) {
        let Value::Array(rows) = listed else {
            return;
        };
        let mut fresh = Vec::new();
        let mut gone = Vec::new();
        {
            let mut book = lock(&self.book);
            let mut seen = BTreeSet::new();
            for row in rows.iter().filter(|r| r.get("tool").is_none()) {
                let Some(shaped) = clean_pending(node, row, &self.find()) else {
                    continue;
                };
                let id = shaped
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                seen.insert(id.clone());
                let skip = book
                    .followed
                    .get(node)
                    .is_none_or(|f| f.gone.contains(&id) || !f.following());
                if !skip && book.put(node, shaped.clone()) {
                    fresh.push(shaped);
                }
            }
            for id in before.difference(&seen) {
                if let Some((_, prompt)) = book.prompts.remove(id) {
                    gone.push(prompt);
                }
            }
        }
        for prompt in gone {
            self.announce(topics::PROMPT_RESOLVED, &resolved_for(&prompt, "unlisted"));
        }
        for prompt in fresh {
            self.announce(topics::PROMPT_PENDING, &prompt);
        }
    }

    fn renew(&self, node: &NodeId, source: &dyn PromptSource, now: u64) {
        let epoch = lock(&self.book).epoch;
        let followed = source.follow(node);
        if lock(&self.book).epoch != epoch {
            return;
        }
        let lapsed = lock(&self.book)
            .followed
            .get(node)
            .and_then(|f| f.ok_at)
            .is_some_and(|at| now >= at + LEASE_MS);
        match followed {
            Ok(()) => {
                if lapsed {
                    self.drop_node(node, "unfollowed");
                }
                {
                    let mut book = lock(&self.book);
                    let f = book.followed.entry(node.clone()).or_default();
                    f.ok_at = Some(now);
                    f.next_at = now + FOLLOW_EVERY_MS;
                }
                self.relist(node, source, now);
            }
            Err(_) => {
                if lapsed {
                    self.drop_node(node, "unfollowed");
                }
                let mut book = lock(&self.book);
                book.followed.entry(node.clone()).or_default().next_at = now + RETRY_MS;
            }
        }
    }

    fn relist(&self, node: &NodeId, source: &dyn PromptSource, now: u64) {
        let (epoch, before) = {
            let mut book = lock(&self.book);
            if let Some(f) = book.followed.get_mut(node) {
                f.listing = true;
                f.gone.clear();
            }
            (book.epoch, book.ids_of(node))
        };
        let answer = source.list(node);
        if lock(&self.book).epoch != epoch {
            return;
        }
        if let Ok(rows) = &answer {
            self.fill(node, rows, &before);
        }
        let mut book = lock(&self.book);
        if let Some(f) = book.followed.get_mut(node) {
            f.listing = false;
            f.gone.clear();
            if answer.is_err() {
                f.next_at = now + RETRY_MS;
            }
        }
    }

    pub fn sync<S: PromptSource + 'static>(
        self: &Arc<Self>,
        peers: &[Peer],
        active: bool,
        source: &Arc<S>,
        now: u64,
    ) {
        if !active {
            self.stop("unfollowed");
            return;
        }
        let online: Vec<&Peer> = peers.iter().filter(|p| p.online).collect();
        let tracked: BTreeSet<NodeId> = {
            let book = lock(&self.book);
            book.followed
                .keys()
                .cloned()
                .chain(book.prompts.values().map(|(n, _)| n.clone()))
                .collect()
        };
        for node in tracked {
            if !online.iter().any(|p| p.id == node) {
                self.drop_node(&node, "offline");
            }
        }
        for peer in online {
            let due = {
                let mut book = lock(&self.book);
                let ready = book.followed.get(&peer.id).is_none_or(|f| now >= f.next_at);
                ready && book.busy.insert(peer.id.clone())
            };
            if !due {
                continue;
            }
            let (me, source, id) = (self.clone(), source.clone(), peer.id.clone());
            (self.spawn)(Box::new(move || {
                me.renew(&id, source.as_ref(), now);
                lock(&me.book).busy.remove(&id);
            }));
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
                let source = Arc::new(NodeSource::new(node.clone()));
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

pub fn leased_clients(status: &Value, nodes: &[String]) -> Vec<NodeId> {
    let members = status.get("members").and_then(Value::as_array);
    nodes
        .iter()
        .filter_map(|n| NodeId::parse(n))
        .filter(|id| {
            members.into_iter().flatten().any(|m| {
                m.get("id").and_then(Value::as_str) == Some(id.as_str())
                    && m.get("state").and_then(Value::as_str) == Some("active")
                    && m.get("client").and_then(Value::as_bool) == Some(true)
            })
        })
        .collect()
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
        let to = leased_clients(&node.status(), nodes);
        if !to.is_empty() {
            node.tell_to(to, topics::PROMPTS, event, data.clone());
        }
    }
}

#[cfg(test)]
#[path = "cluster_prompts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_prompts_fix_tests.rs"]
mod fix_tests;
