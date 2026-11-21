use crate::cluster_node::{ClusterNode, LocalCalls};
use crate::inbox_mirrors::{Home, InboxLink, Refusal};
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::versions::FEATURE_INBOX_CONSENT;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{LocalCmd, NodeId};
use notchling_engine::inbox::{ActionId, Change, Item, Kind};
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

pub const NODE_INBOX: &str = "node.inbox";
pub const NODE_OPS: [&str; 1] = [NODE_INBOX];
pub const FORWARD_WAIT: Duration = Duration::from_secs(5);
pub const LIST_WAIT: Duration = Duration::from_secs(3);
pub const RESYNC_MS: i64 = 60_000;
pub const BACKOFF_MS: i64 = 5_000;
pub const BACKOFF_MAX_MS: i64 = 300_000;
pub const CLOSE_WAIT: Duration = Duration::from_secs(1);
pub const KEEP_MISSES: u32 = 2;
pub const QUEUE_MAX: usize = 1_000;
const LOOP_EVERY: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub id: NodeId,
    pub name: String,
    pub online: bool,
    pub keeps: bool,
    pub consent: bool,
}

pub trait Net: Send + Sync {
    fn me(&self) -> Option<NodeId>;
    fn home(&self) -> Option<NodeId>;
    fn peers(&self) -> Vec<Peer>;
    fn is_client(&self) -> bool;
    fn keeps(&self) -> bool;
    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        tier: OriginTier,
        within: Duration,
    ) -> Result<Value, ApiError>;
}

pub type Holder = Arc<OnceLock<Arc<ClusterNode>>>;

pub struct NodeNet {
    holder: Holder,
}

impl NodeNet {
    pub fn new(holder: Holder) -> NodeNet {
        NodeNet { holder }
    }

    fn status(&self) -> Option<Value> {
        self.holder.get().map(|node| node.status())
    }

    fn member(&self, id: &NodeId) -> Option<Value> {
        self.status()?
            .get("members")?
            .as_array()?
            .iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .cloned()
    }
}

impl Net for NodeNet {
    fn me(&self) -> Option<NodeId> {
        self.holder.get().map(|node| node.me().clone())
    }

    fn home(&self) -> Option<NodeId> {
        let status = self.status()?;
        NodeId::parse(status.pointer("/cluster/home")?.as_str()?)
    }

    fn peers(&self) -> Vec<Peer> {
        let Some(status) = self.status() else {
            return Vec::new();
        };
        let me = status.get("me").and_then(Value::as_str);
        let flag = |m: &Value, key: &str| m.get(key).and_then(Value::as_bool).unwrap_or(false);
        status
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|m| m.get("state").and_then(Value::as_str) == Some("active"))
            .filter_map(|m| {
                let id = NodeId::parse(m.get("id")?.as_str()?)?;
                (Some(id.as_str()) != me).then(|| Peer {
                    name: m
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    online: flag(m, "online"),
                    keeps: flag(m, "canBeMaster")
                        || m.get("role").and_then(Value::as_str) == Some("home"),
                    consent: knows_consent(m),
                    id,
                })
            })
            .collect()
    }

    fn is_client(&self) -> bool {
        self.me()
            .and_then(|me| self.member(&me))
            .is_some_and(|m| m.get("client").and_then(Value::as_bool).unwrap_or(false))
    }

    fn keeps(&self) -> bool {
        let Some(me) = self.me() else {
            return false;
        };
        self.home().as_ref() == Some(&me)
            || self
                .member(&me)
                .is_some_and(|m| m.get("canBeMaster").and_then(Value::as_bool) == Some(true))
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        tier: OriginTier,
        within: Duration,
    ) -> Result<Value, ApiError> {
        let node = self
            .holder
            .get()
            .ok_or_else(|| failed("the cluster node isn't up"))?;
        let origin = Origin {
            node: node.me().clone(),
            tier,
        };
        node.local_within(
            LocalCmd::Forward {
                node: to.clone(),
                op: op.to_string(),
                args: args.into(),
                origin,
            },
            within,
        )
    }
}

pub struct NetLink {
    net: Arc<dyn Net>,
}

impl NetLink {
    pub fn new(net: Arc<dyn Net>) -> NetLink {
        NetLink { net }
    }
}

impl InboxLink for NetLink {
    fn me(&self) -> Option<NodeId> {
        self.net.me()
    }

    fn answer(
        &self,
        owner: &NodeId,
        id: &str,
        action: ActionId,
        by: &str,
    ) -> Result<Value, ApiError> {
        let peer = self.net.peers().into_iter().find(|p| p.id == *owner);
        match peer {
            None => return Err(failed("That machine isn't in the cluster.")),
            Some(p) if !p.online => {
                return Err(failed(format!(
                    "{} is offline, so it can't take the answer.",
                    p.name
                )))
            }
            Some(_) => {}
        }
        let args = json!({"id": id, "action": action, "by": by});
        self.net.call(
            owner,
            ops::INBOX_ANSWER,
            args,
            OriginTier::App,
            FORWARD_WAIT,
        )
    }

    fn home_items(&self) -> Home {
        let (Some(me), Some(home)) = (self.net.me(), self.net.home()) else {
            return Home::Off;
        };
        if home == me || !self.net.is_client() {
            return Home::Off;
        }
        let up = self
            .net
            .peers()
            .into_iter()
            .any(|p| p.id == home && p.online);
        if !up {
            return Home::Off;
        }
        let list = self
            .net
            .call(
                &home,
                ops::INBOX_LIST,
                json!({}),
                OriginTier::App,
                LIST_WAIT,
            )
            .ok()
            .and_then(|list| list.get("items").and_then(Value::as_array).cloned());
        let Some(items) = list else {
            return Home::Unreachable;
        };
        let items = items
            .iter()
            .filter_map(|item| serde_json::from_value::<Item>(item.clone()).ok())
            .collect();
        Home::Items(home, items)
    }
}

fn refused(why: Refusal) -> ApiError {
    let code = match why {
        Refusal::Foreign => ErrorCode::Forbidden,
        _ => ErrorCode::BadRequest,
    };
    ApiError::new(code, why.text())
}

pub struct InboxCalls {
    inner: Arc<dyn LocalCalls>,
    net: Arc<dyn Net>,
    inbox: Arc<InboxOps>,
}

impl InboxCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, net: Arc<dyn Net>, inbox: Arc<InboxOps>) -> InboxCalls {
        InboxCalls { inner, net, inbox }
    }

    fn take(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        if origin.tier != OriginTier::Node || self.net.me().as_ref() == Some(&origin.node) {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only another node sends its inbox items",
            ));
        }
        if !self.net.keeps() {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "this node keeps no other node's inbox items",
            ));
        }
        let owner = &origin.node;
        let item_of = |value: &Value| {
            serde_json::from_value::<Item>(value.clone())
                .map_err(|_| ApiError::bad_request("that isn't an inbox item"))
        };
        let event = args.get("event").and_then(Value::as_str).unwrap_or("");
        match event {
            "changed" => {
                let item = item_of(args.get("item").unwrap_or(&Value::Null))?;
                self.inbox.mirror_changed(owner, item).map_err(refused)?;
            }
            "removed" => {
                let id = args
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::bad_request("removed takes an id"))?;
                self.inbox.mirror_removed(owner, id).map_err(refused)?;
            }
            "sync" => {
                let list = args
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| ApiError::bad_request("sync takes a list of items"))?;
                let items = list.iter().map(item_of).collect::<Result<Vec<_>, _>>()?;
                self.inbox.mirror_sync(owner, items).map_err(refused)?;
            }
            _ => {
                return Err(ApiError::bad_request(
                    "node.inbox takes changed, removed or sync",
                ))
            }
        }
        Ok(json!({"ok": true}))
    }
}

impl LocalCalls for InboxCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_INBOX => self.take(args, origin),
            _ => self.inner.call(op, args, origin),
        }
    }
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Default)]
struct Queue {
    changes: Vec<Change>,
    overflow: bool,
    synced: BTreeMap<NodeId, i64>,
    held_off: BTreeMap<NodeId, i64>,
    fails: BTreeMap<NodeId, u32>,
    last_targets: Vec<NodeId>,
    stop: bool,
}

pub struct Outbox {
    net: Arc<dyn Net>,
    inbox: Arc<InboxOps>,
    clock: Clock,
    queue: Mutex<Queue>,
    wake: Condvar,
    follow: Mutex<Option<Arc<crate::cluster_inbox_follow::Follow>>>,
    misses: Mutex<u32>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn id_of(change: &Change) -> &str {
    match change {
        Change::Raised(item) | Change::Updated(item) => &item.id,
        Change::Removed(id) => id,
    }
}

fn coalesced(changes: Vec<Change>) -> Vec<Change> {
    let mut last: BTreeMap<String, usize> = BTreeMap::new();
    for (at, change) in changes.iter().enumerate() {
        last.insert(id_of(change).to_string(), at);
    }
    changes
        .into_iter()
        .enumerate()
        .filter(|(at, change)| last.get(id_of(change)) == Some(at))
        .map(|(_, change)| change)
        .collect()
}

pub(crate) fn knows_consent(member: &Value) -> bool {
    member
        .pointer("/versions/features")
        .and_then(Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|f| f.as_str() == Some(FEATURE_INBOX_CONSENT))
        })
}

fn shown_to_peers(item: &Item, knows: bool) -> Item {
    let mut shown = item.clone();
    shown.open = None;
    if shown.kind == Kind::Consent && !knows {
        shown.kind = Kind::Notice;
    }
    shown
}

fn event_of(change: &Change, knows: bool) -> Value {
    match change {
        Change::Raised(item) | Change::Updated(item) => {
            json!({"event": "changed", "item": shown_to_peers(item, knows)})
        }
        Change::Removed(id) => json!({"event": "removed", "id": id}),
    }
}

impl Outbox {
    pub fn new(net: Arc<dyn Net>, inbox: Arc<InboxOps>, clock: Clock) -> Arc<Outbox> {
        Arc::new(Outbox {
            net,
            inbox,
            clock,
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            follow: Mutex::new(None),
            misses: Mutex::new(0),
            thread: Mutex::new(None),
        })
    }

    pub fn follow(&self, follow: Arc<crate::cluster_inbox_follow::Follow>) {
        *lock(&self.follow) = Some(follow);
    }

    pub fn tap(self: &Arc<Self>) -> crate::inbox_mirrors::Tap {
        let outbox = Arc::downgrade(self);
        Arc::new(move |changes| {
            if let Some(outbox) = outbox.upgrade() {
                outbox.push(changes);
            }
        })
    }

    pub fn push(&self, changes: Vec<Change>) {
        let mut queue = lock(&self.queue);
        if queue.changes.len() + changes.len() > QUEUE_MAX {
            queue.changes.clear();
            queue.overflow = true;
        } else {
            queue.changes.extend(changes);
        }
        self.wake.notify_all();
    }

    fn targets(&self) -> Vec<NodeId> {
        self.net
            .peers()
            .into_iter()
            .filter(|p| p.online && p.keeps)
            .map(|p| p.id)
            .collect()
    }

    fn sync_to(&self, target: &NodeId, knows: bool) -> Result<(), ApiError> {
        let items: Vec<Item> = self
            .inbox
            .own_open()
            .iter()
            .map(|item| shown_to_peers(item, knows))
            .collect();
        let args = json!({"event": "sync", "items": items});
        self.net
            .call(target, NODE_INBOX, args, OriginTier::Node, FORWARD_WAIT)
            .map(|_| ())
    }

    fn keep_mirrors(&self) {
        if self.net.me().is_none() {
            return;
        }
        let owners: BTreeSet<NodeId> = if self.net.keeps() {
            *lock(&self.misses) = 0;
            self.net.peers().into_iter().map(|p| p.id).collect()
        } else {
            let mut misses = lock(&self.misses);
            *misses += 1;
            if *misses < KEEP_MISSES {
                return;
            }
            BTreeSet::new()
        };
        self.inbox.keep_mirrors(&owners);
    }

    pub fn flush(&self) {
        let now = (self.clock)();
        self.keep_mirrors();
        let targets = self.targets();
        let batch = {
            let mut queue = lock(&self.queue);
            let overflow = std::mem::take(&mut queue.overflow);
            if queue.last_targets != targets {
                queue.last_targets = targets.clone();
                queue.held_off.clear();
                queue.fails.clear();
            }
            queue.synced.retain(|id, at| {
                targets.contains(id) && !overflow && now.saturating_sub(*at) < RESYNC_MS
            });
            queue.held_off.retain(|_, until| *until > now);
            coalesced(std::mem::take(&mut queue.changes))
        };
        let ready: BTreeSet<NodeId> = lock(&self.queue).synced.keys().cloned().collect();
        let knowing: BTreeSet<NodeId> = self
            .net
            .peers()
            .into_iter()
            .filter(|p| p.consent)
            .map(|p| p.id)
            .collect();
        for target in &targets {
            let knows = knowing.contains(target);
            if lock(&self.queue).held_off.contains_key(target) {
                continue;
            }
            if !ready.contains(target) {
                let result = self.sync_to(target, knows);
                self.settle(target, result.is_ok(), now);
                continue;
            }
            for change in &batch {
                let sent = self.net.call(
                    target,
                    NODE_INBOX,
                    event_of(change, knows),
                    OriginTier::Node,
                    FORWARD_WAIT,
                );
                if sent.is_err() {
                    self.settle(target, false, now);
                    break;
                }
            }
        }
    }

    fn settle(&self, target: &NodeId, ok: bool, now: i64) {
        let mut queue = lock(&self.queue);
        if ok {
            queue.synced.insert(target.clone(), now);
            queue.held_off.remove(target);
            queue.fails.remove(target);
        } else {
            queue.synced.remove(target);
            let fails = queue.fails.entry(target.clone()).or_insert(0);
            let wait = (BACKOFF_MS << (*fails).min(7)).min(BACKOFF_MAX_MS);
            *fails += 1;
            queue
                .held_off
                .insert(target.clone(), now.saturating_add(wait));
        }
    }

    pub fn spawn(self: &Arc<Self>) {
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("cluster-inbox".into())
            .spawn(move || loop {
                me.flush();
                let follow = lock(&me.follow).clone();
                if let Some(follow) = follow {
                    follow.poll((me.clock)());
                }
                let queue = lock(&me.queue);
                if queue.stop {
                    return;
                }
                let (queue, _) = me
                    .wake
                    .wait_timeout(queue, LOOP_EVERY)
                    .unwrap_or_else(|e| e.into_inner());
                if queue.stop {
                    return;
                }
            });
        *lock(&self.thread) = spawned.ok();
    }

    pub fn close(&self) {
        lock(&self.queue).stop = true;
        self.wake.notify_all();
        let thread = lock(&self.thread).take();
        let Some(handle) = thread else {
            return;
        };
        let deadline = std::time::Instant::now() + CLOSE_WAIT;
        while !handle.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[path = "cluster_inbox_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_inbox_guard_tests.rs"]
mod guard_tests;

#[cfg(test)]
#[path = "cluster_inbox_fix_tests.rs"]
mod fix_tests;
