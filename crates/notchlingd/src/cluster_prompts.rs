use crate::acting_ops::Room;
use crate::cluster_forward::{PromptPush, NODE_FOLLOW};
use crate::cluster_node::ClusterNode;
use crate::cluster_prompts_book::{Book, Ticket};
use crate::cluster_prompts_clean::{clean_pending, clean_resolved, Found};
use crate::cluster_sessions::{is_client, peers_of, NodeSource, Peer, RemoteSessions};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

pub use crate::cluster_prompts_book::{GONE_MAX, MAX_PER_NODE, MAX_TOTAL, REMOTE_BYTES};
pub use crate::cluster_prompts_clean::{ID_MAX, INPUT_MAX, QUESTION_MAX, TEXT_MAX};

pub const FOLLOW_EVERY_MS: u64 = 30_000;
pub const LEASE_MS: u64 = crate::cluster_forward::FOLLOW_LEASE_MS;
pub const RETRY_MS: u64 = 5_000;
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
pub type Spawn = Arc<dyn Fn(Job) -> std::io::Result<()> + Send + Sync>;
pub type Ticks = Arc<dyn Fn() -> u64 + Send + Sync>;

fn threaded() -> Spawn {
    Arc::new(|job: Job| {
        std::thread::Builder::new()
            .name("cluster-prompts-ask".into())
            .spawn(job)
            .map(|_| ())
    })
}

fn monotonic() -> Ticks {
    let start = Instant::now();
    Arc::new(move || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX))
}

#[derive(Clone, Copy)]
struct At {
    now: u64,
    tick: u64,
}

impl At {
    fn later(&self, tick: u64) -> u64 {
        self.now.saturating_add(tick.saturating_sub(self.tick))
    }
}

pub struct RemotePrompts {
    room: Arc<dyn Room>,
    mirror: Arc<RemoteSessions>,
    book: Mutex<Book>,
    spawn: Spawn,
    ticks: Ticks,
}

struct Free<'a>(&'a Mutex<Book>, &'a NodeId);

impl Drop for Free<'_> {
    fn drop(&mut self) {
        lock(self.0).busy.remove(self.1);
    }
}

struct Draining<'a>(&'a Mutex<Book>);

impl Drop for Draining<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            lock(self.0).release();
        }
    }
}

impl RemotePrompts {
    pub fn new(room: Arc<dyn Room>, mirror: Arc<RemoteSessions>) -> Arc<RemotePrompts> {
        RemotePrompts::with_seams(room, mirror, threaded(), monotonic())
    }

    pub fn with_spawn(
        room: Arc<dyn Room>,
        mirror: Arc<RemoteSessions>,
        spawn: Spawn,
    ) -> Arc<RemotePrompts> {
        RemotePrompts::with_seams(room, mirror, spawn, monotonic())
    }

    pub fn with_seams(
        room: Arc<dyn Room>,
        mirror: Arc<RemoteSessions>,
        spawn: Spawn,
        ticks: Ticks,
    ) -> Arc<RemotePrompts> {
        Arc::new(RemotePrompts {
            room,
            mirror,
            book: Mutex::new(Book::default()),
            spawn,
            ticks,
        })
    }

    fn change<R>(&self, edit: impl FnOnce(&mut Book) -> R) -> R {
        let out = edit(&mut lock(&self.book));
        self.flush();
        out
    }

    fn flush(&self) {
        if !lock(&self.book).claim() {
            return;
        }
        let _guard = Draining(&self.book);
        loop {
            let next = lock(&self.book).next_event();
            let Some((event, data)) = next else {
                return;
            };
            self.room.publish(topics::PROMPTS, event, &data);
            lock(&self.book).published();
        }
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
        Self::rows_of(&lock(&self.book))
    }

    fn rows_of(book: &Book) -> Vec<Value> {
        let mut rows: Vec<Value> = book.prompts.values().map(|(_, p)| p.clone()).collect();
        rows.sort_by_key(|p| p.get("createdMs").and_then(Value::as_i64).unwrap_or(0));
        rows
    }

    pub fn merge(&self, local: Value, caller: &Caller) -> Value {
        if caller.client == ClientKind::Node {
            return local;
        }
        let rows = match local {
            Value::Array(rows) => rows,
            _ => Vec::new(),
        };
        let (remote, held) = {
            let book = lock(&self.book);
            (Self::rows_of(&book), book.held_bytes())
        };
        let budget = crate::prompts::LIST_BYTES.saturating_sub(held);
        let mut rows = crate::prompts::fit_within(rows, budget);
        rows.extend(remote);
        Value::Array(rows)
    }

    pub fn drop_node(&self, node: &NodeId, why: &str) {
        self.change(|book| book.drop_node(node, why));
    }

    pub fn stop(&self, why: &str) {
        self.change(|book| book.stop(why));
    }

    pub fn on_event(&self, from: &NodeId, event: &str, data: &Value) {
        match event {
            topics::PROMPT_PENDING => {
                let Some(shaped) = clean_pending(from, data, &self.find()) else {
                    return;
                };
                self.change(|book| book.pending(from, shaped));
            }
            topics::PROMPT_RESOLVED => {
                let Some((id, shaped)) = clean_resolved(from, data) else {
                    return;
                };
                self.change(|book| book.resolved(from, &id, shaped));
            }
            _ => {}
        }
    }

    fn renew(&self, node: &NodeId, source: &dyn PromptSource, at: At, ticket: Ticket) {
        if !lock(&self.book).current(node, ticket) {
            return;
        }
        let followed = source.follow(node);
        let done = at.later((self.ticks)());
        let next = self.change(|book| {
            if !book.current(node, ticket) {
                return None;
            }
            let lapsed = book
                .followed
                .get(node)
                .and_then(|f| f.ok_at)
                .is_some_and(|ok| done >= ok.saturating_add(LEASE_MS));
            if lapsed {
                book.drop_node(node, "unfollowed");
            }
            let fresh = book.ticket(node);
            let f = book.followed.entry(node.clone()).or_default();
            match followed {
                Ok(()) => {
                    f.ok_at = Some(at.now);
                    f.next_at = at.now + FOLLOW_EVERY_MS;
                    Some(fresh)
                }
                Err(_) => {
                    f.next_at = done + RETRY_MS;
                    None
                }
            }
        });
        if let Some(ticket) = next {
            self.relist(node, source, at, ticket);
        }
    }

    fn relist(&self, node: &NodeId, source: &dyn PromptSource, at: At, ticket: Ticket) {
        let before = {
            let mut book = lock(&self.book);
            if !book.current(node, ticket) {
                return;
            }
            let Some(f) = book.followed.get_mut(node) else {
                return;
            };
            f.listing = true;
            f.gone.clear();
            book.ids_of(node)
        };
        let answer = source.list(node);
        let done = at.later((self.ticks)());
        let find = self.find();
        self.change(|book| {
            if book.current(node, ticket) {
                match &answer {
                    Ok(rows) => book.fill(node, rows, &before, &find),
                    Err(_) => {
                        if let Some(f) = book.followed.get_mut(node) {
                            f.next_at = done + RETRY_MS;
                        }
                    }
                }
            }
            if let Some(f) = book.followed.get_mut(node) {
                f.listing = false;
                f.gone.clear();
            }
        });
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
        let online: BTreeSet<&NodeId> = peers.iter().filter(|p| p.online).map(|p| &p.id).collect();
        self.change(|book| {
            for node in book.tracked() {
                if !online.contains(&node) {
                    book.drop_node(&node, "offline");
                }
            }
        });
        let at = At {
            now,
            tick: (self.ticks)(),
        };
        for id in online {
            let ticket = {
                let mut book = lock(&self.book);
                let ready = book.followed.get(id).is_none_or(|f| now >= f.next_at);
                if !(ready && book.busy.insert(id.clone())) {
                    continue;
                }
                book.ticket(id)
            };
            let (me, source, node) = (self.clone(), source.clone(), id.clone());
            let job: Job = Box::new(move || {
                let _free = Free(&me.book, &node);
                me.renew(&node, source.as_ref(), at, ticket);
            });
            if (self.spawn)(job).is_err() {
                let mut book = lock(&self.book);
                book.busy.remove(id);
                book.followed.entry(id.clone()).or_default().next_at = now + RETRY_MS;
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
            node.tell_clients(to, topics::PROMPTS, event, data.clone());
        }
    }
}

#[cfg(test)]
#[path = "cluster_prompts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_prompts_fix_tests.rs"]
mod fix_tests;

#[cfg(test)]
#[path = "cluster_prompts_race_tests.rs"]
mod race_tests;

#[cfg(test)]
#[path = "cluster_prompts_budget_tests.rs"]
mod budget_tests;
