use crate::acting_ops::{now_ms, Room};
use crate::inbox_events::events_of;
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use notchling_engine::bridge::inbox::InboxOutcome;
use notchling_engine::cluster::persist::{self as cluster_persist, CLUSTER_DB};
use notchling_engine::inbox::{self, ActionId, Book, Change, Item, Kind, State, Store};
use notchling_engine::store;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(1);
const LEAVE_WAIT: Duration = Duration::from_secs(5);
const ANSWERED_BY: &str = "mac";
const MAY_ALLOW: [&str; 2] = ["mac", "phone"];

pub trait Responder: Send + Sync {
    fn answered(&self, item: &Item, action: ActionId) -> bool;

    fn answered_by(&self, item: &Item, action: ActionId, _by: &str) -> bool {
        self.answered(item, action)
    }
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

enum Msg {
    Changes(Vec<Change>),
    Publish(Vec<Change>),
    Flush(Sender<()>),
    Move(Store, Option<Sender<bool>>),
}

struct Inner {
    book: Book,
    responders: HashMap<String, Arc<dyn Responder>>,
    writes: Option<Sender<Msg>>,
    events: Option<Events>,
    paused: Option<Vec<Change>>,
}

impl Inner {
    fn note(&self, changes: &[Change]) {
        if let Some(events) = &self.events {
            for event in events_of(changes) {
                events.push(event);
            }
        }
    }

    fn send(&mut self, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        if let Some(paused) = &mut self.paused {
            paused.extend(changes);
            return;
        }
        if let Some(writes) = &self.writes {
            let _ = writes.send(Msg::Changes(changes));
        }
    }

    fn publish(&self, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        if let Some(writes) = &self.writes {
            let _ = writes.send(Msg::Publish(changes));
        }
    }

    fn forget(&mut self, changes: &[Change]) {
        for change in changes {
            match change {
                Change::Removed(id) => {
                    self.responders.remove(id);
                }
                Change::Updated(item) if item.state != State::Open => {
                    self.responders.remove(&item.id);
                }
                _ => {}
            }
        }
    }
}

pub struct InboxOps {
    clock: Clock,
    data_dir: PathBuf,
    cluster_dir: Mutex<Option<PathBuf>>,
    store: Arc<Mutex<Store>>,
    inner: Mutex<Inner>,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

fn write_loop(
    reads: Receiver<Msg>,
    data_dir: PathBuf,
    store: Arc<Mutex<Store>>,
    room: Arc<dyn Room>,
) {
    while let Ok(msg) = reads.recv() {
        match msg {
            Msg::Changes(changes) => {
                for change in &changes {
                    let at = lock(&store).clone();
                    persist(&at, change);
                    publish(room.as_ref(), change);
                }
            }
            Msg::Publish(changes) => {
                for change in &changes {
                    publish(room.as_ref(), change);
                }
            }
            Msg::Flush(ack) => {
                let _ = ack.send(());
            }
            Msg::Move(to, ack) => {
                let moved = relocate(&store, &data_dir, to);
                if let Some(ack) = ack {
                    let _ = ack.send(moved);
                }
            }
        }
    }
}

fn relocate(store: &Mutex<Store>, data_dir: &Path, to: Store) -> bool {
    let from = lock(store).clone();
    if from == to {
        return true;
    }
    let node_db = store::db_path(data_dir);
    let moved = match (&from, &to) {
        (Store::Node(_), Store::Cluster(dir)) => {
            inbox::migrate_inbox(&node_db, &dir.join(CLUSTER_DB))
        }
        (Store::Cluster(dir), Store::Node(_)) => {
            inbox::migrate_inbox_back(&dir.join(CLUSTER_DB), &node_db)
        }
        _ => Ok(0),
    };
    match moved {
        Ok(_) => {
            *lock(store) = to;
            true
        }
        Err(why) => {
            report(Err(format!("moving it failed: {why}")));
            if matches!(to, Store::Node(_)) {
                *lock(store) = to;
            }
            false
        }
    }
}

fn persist(at: &Store, change: &Change) {
    report(inbox::save_to(at, change));
}

fn report(done: Result<(), String>) {
    if let Err(why) = done {
        eprintln!("notchling: saving the inbox failed ({why})");
    }
}

fn publish(room: &dyn Room, change: &Change) {
    match change {
        Change::Raised(item) | Change::Updated(item) => {
            room.publish(topics::INBOX, topics::INBOX_CHANGED, &json!({"item": item}));
        }
        Change::Removed(id) => {
            room.publish(topics::INBOX, topics::INBOX_REMOVED, &json!({"id": id}));
        }
    }
}

#[cfg(unix)]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let at = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((at.as_nanos() as u64, meta.len()))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(message: &str) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, message)
}

fn state_name(state: State) -> Value {
    serde_json::to_value(state).unwrap_or(Value::Null)
}

fn answer_json(answer: Option<ActionId>) -> Value {
    serde_json::to_value(answer).unwrap_or(Value::Null)
}

impl InboxOps {
    pub fn new(data_dir: PathBuf, room: Arc<dyn Room>) -> Arc<InboxOps> {
        InboxOps::with_clock(data_dir, room, Arc::new(now_ms))
    }

    pub fn with_clock(data_dir: PathBuf, room: Arc<dyn Room>, clock: Clock) -> Arc<InboxOps> {
        let (writes, reads) = channel::<Msg>();
        let folder = data_dir.clone();
        let store = Arc::new(Mutex::new(Store::Node(data_dir.clone())));
        let writing = store.clone();
        let writer = std::thread::Builder::new()
            .name("notchling-inbox-writes".into())
            .spawn(move || write_loop(reads, folder, writing, room))
            .ok();
        let inner = Inner {
            book: Book::new(),
            responders: HashMap::new(),
            writes: Some(writes),
            events: None,
            paused: None,
        };
        let ops = InboxOps {
            clock,
            data_dir,
            cluster_dir: Mutex::new(None),
            store,
            inner: Mutex::new(inner),
            stop: Mutex::new(None),
            thread: Mutex::new(None),
            writer: Mutex::new(writer),
        };
        Arc::new(ops)
    }

    pub fn with_events(self: Arc<Self>, events: Events) -> Arc<InboxOps> {
        lock(&self.inner).events = Some(events);
        self
    }

    pub fn with_cluster_dir(self: Arc<Self>, dir: PathBuf) -> Arc<InboxOps> {
        *lock(&self.cluster_dir) = Some(dir);
        self
    }

    pub fn join_cluster(&self) {
        let dir = lock(&self.cluster_dir).clone();
        if let Some(dir) = dir {
            self.send_move(Store::Cluster(dir), None);
        }
    }

    pub fn leave_cluster(&self) -> bool {
        self.leave_cluster_within(LEAVE_WAIT)
    }

    pub(crate) fn leave_cluster_within(&self, wait: Duration) -> bool {
        let (ack_tx, ack_rx) = channel::<bool>();
        let sent = self.send_move(Store::Node(self.data_dir.clone()), Some(ack_tx));
        let moved = sent && ack_rx.recv_timeout(wait).unwrap_or(false);
        *lock(&self.store) = Store::Node(self.data_dir.clone());
        moved
    }

    fn send_move(&self, to: Store, ack: Option<Sender<bool>>) -> bool {
        match &lock(&self.inner).writes {
            Some(writes) => writes.send(Msg::Move(to, ack)).is_ok(),
            None => false,
        }
    }

    fn settle_store(&self, now: i64) {
        let Some(dir) = lock(&self.cluster_dir).clone() else {
            return;
        };
        match cluster_persist::load(&dir) {
            Ok(Some(_)) => {
                self.join_cluster();
                self.flush();
            }
            Ok(None) => {
                let back = inbox::migrate_inbox_back(
                    &dir.join(CLUSTER_DB),
                    &store::db_path(&self.data_dir),
                );
                report(back.map(|_| ()));
            }
            Err(_) => {
                let at = Store::Cluster(dir);
                if inbox::load_from(&at, now).is_ok_and(|book| !book.items().is_empty()) {
                    *lock(&self.store) = at;
                }
            }
        }
    }

    pub fn load(&self) {
        let now = (self.clock)();
        self.settle_store(now);
        let at = lock(&self.store).clone();
        let saved = inbox::load_from(&at, now).unwrap_or_else(|why| {
            eprintln!("notchling: reading the inbox failed ({why}); starting it empty");
            Book::new()
        });
        let withdrawn: Vec<Change> = saved
            .items()
            .into_iter()
            .filter(|item| item.state == State::Withdrawn && item.closed_ms == Some(now))
            .map(Change::Updated)
            .collect();
        let mut inner = lock(&self.inner);
        inner.book = saved;
        inner.responders.clear();
        inner.note(&withdrawn);
        inner.send(withdrawn);
    }

    pub fn raise(&self, item: Item, responder: Option<Arc<dyn Responder>>) -> String {
        let id = item.id.clone();
        {
            let mut inner = lock(&self.inner);
            let changes = inner.book.raise(item);
            let open = changes.iter().any(|change| match change {
                Change::Raised(item) | Change::Updated(item) => {
                    item.id == id && item.state == State::Open
                }
                Change::Removed(_) => false,
            });
            if let (true, Some(responder)) = (open, responder) {
                inner.responders.insert(id.clone(), responder);
            }
            inner.forget(&changes);
            inner.note(&changes);
            inner.send(changes);
        }
        id
    }

    pub fn resolve(&self, id: &str, state: State, answer: Option<ActionId>, by: Option<&str>) {
        let now = (self.clock)();
        let mut inner = lock(&self.inner);
        let changes: Vec<Change> = inner
            .book
            .resolve(id, state, answer, by, now)
            .into_iter()
            .collect();
        inner.responders.remove(id);
        inner.note(&changes);
        inner.send(changes);
    }

    pub fn tick(&self, now_ms: i64) {
        let mut inner = lock(&self.inner);
        let Inner {
            book, responders, ..
        } = &mut *inner;
        let mut changes = book.expire_except(now_ms, &|id| responders.contains_key(id));
        let before: Vec<String> = inner.book.items().into_iter().map(|i| i.id).collect();
        inner.book.prune(now_ms);
        let after: HashSet<String> = inner.book.items().into_iter().map(|i| i.id).collect();
        changes.extend(
            before
                .into_iter()
                .filter(|id| !after.contains(id))
                .map(Change::Removed),
        );
        inner.forget(&changes);
        inner.note(&changes);
        inner.send(changes);
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        self.spawn_loop_every(TICK);
    }

    pub fn spawn_loop_every(self: &Arc<Self>, every: Duration) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let (stop_tx, stop_rx) = channel::<()>();
        *lock(&self.stop) = Some(stop_tx);
        let ops = self.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-inbox".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(every) {
                    ops.tick((ops.clock)());
                }
            })
            .ok();
    }

    pub fn flush(&self) {
        let (ack_tx, ack_rx) = channel::<()>();
        let sent = match &lock(&self.inner).writes {
            Some(writes) => writes.send(Msg::Flush(ack_tx)).is_ok(),
            None => false,
        };
        if sent {
            let _ = ack_rx.recv();
        }
    }

    pub fn across_db_swap<T>(&self, db: &Path, swap: impl FnOnce() -> T) -> T {
        lock(&self.inner).paused = Some(Vec::new());
        self.flush();
        let before = file_id(db);
        let out = swap();
        let replaced = file_id(db) != before;
        let mut inner = lock(&self.inner);
        let waiting = inner.paused.take().unwrap_or_default();
        if replaced {
            self.adopt(&mut inner, &waiting);
        } else {
            inner.send(waiting);
        }
        out
    }

    fn adopt(&self, inner: &mut Inner, waiting: &[Change]) {
        let at = lock(&self.store).clone();
        let mut items = inbox::read_from(&at).unwrap_or_else(|why| {
            report(Err(format!("reading the replaced inbox failed: {why}")));
            Vec::new()
        });
        let local = inner.book.items();
        let stored: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        let touched: HashSet<&str> = waiting
            .iter()
            .filter_map(|change| match change {
                Change::Raised(item) | Change::Updated(item) => Some(item.id.as_str()),
                Change::Removed(_) => None,
            })
            .collect();
        let mut writes = Vec::new();
        for item in &local {
            let carried = item.state == State::Open || touched.contains(item.id.as_str());
            if carried && !stored.contains(&item.id) {
                items.push(item.clone());
                writes.push(Change::Raised(item.clone()));
            }
        }
        let now: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        let mut shown: Vec<Change> = items
            .iter()
            .filter(|item| !local.contains(item))
            .map(|item| match local.iter().any(|l| l.id == item.id) {
                true => Change::Updated(item.clone()),
                false => Change::Raised(item.clone()),
            })
            .collect();
        shown.extend(
            local
                .iter()
                .filter(|l| !now.contains(&l.id))
                .map(|l| Change::Removed(l.id.clone())),
        );
        let open: HashSet<String> = items
            .iter()
            .filter(|i| i.state == State::Open)
            .map(|i| i.id.clone())
            .collect();
        inner.responders.retain(|id, _| open.contains(id));
        inner.book = Book::from_items(items);
        inner.note(&shown);
        inner.publish(shown);
        inner.send(writes);
    }

    pub fn close(&self) {
        let stop = lock(&self.stop).take();
        drop(stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
        let writes = lock(&self.inner).writes.take();
        drop(writes);
        let writer = lock(&self.writer).take();
        if let Some(handle) = writer {
            let _ = handle.join();
        }
    }

    fn list(&self) -> Value {
        let inner = lock(&self.inner);
        json!({"items": inner.book.items(), "unread": inner.book.unread()})
    }

    fn seen(&self, args: &Value) -> Result<Value, ApiError> {
        let ids: Vec<String> = args
            .get("ids")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("ids must be a list"))?
            .iter()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect();
        let mut inner = lock(&self.inner);
        let changes = inner.book.mark_seen(&ids);
        inner.send(changes);
        Ok(json!({"unread": inner.book.unread()}))
    }

    pub fn items(&self) -> Vec<Item> {
        lock(&self.inner).book.items()
    }

    pub fn decide(&self, id: &str, action: ActionId, by: &str) -> Result<InboxOutcome, ApiError> {
        let now = (self.clock)();
        let mut inner = lock(&self.inner);
        let current = inner
            .book
            .items()
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "No such inbox item."))?;
        if current.state != State::Open {
            return Ok(refused(&current));
        }
        if !current.actions.iter().any(|offered| offered.id == action) {
            return Err(bad("The item doesn't offer that action."));
        }
        if allows_secret(&current, action) && !MAY_ALLOW.contains(&by) {
            let what = match current.kind {
                Kind::NodeJoin => "a machine into the cluster",
                _ => "a secret",
            };
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                format!("Only the Mac or a paired phone may allow {what}."),
            ));
        }
        if let Some(responder) = inner.responders.get(id).cloned() {
            if !responder.answered_by(&current, action, by) {
                return Ok(refused(&current));
            }
        }
        match inner.book.answer(id, action, by, now) {
            Ok(change) => {
                inner.note(std::slice::from_ref(&change));
                inner.send(vec![change]);
                inner.responders.remove(id);
                Ok(InboxOutcome {
                    answered: true,
                    state: State::Answered,
                    answer: Some(action),
                })
            }
            Err(_) => Ok(refused(&current)),
        }
    }

    fn answer(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("id is required"))?;
        let action: ActionId =
            serde_json::from_value(args.get("action").cloned().unwrap_or(Value::Null))
                .map_err(|_| bad("action is not one the inbox knows"))?;
        let outcome = self.decide(id, action, ANSWERED_BY)?;
        Ok(json!({
            "answered": outcome.answered,
            "state": state_name(outcome.state),
            "answer": answer_json(outcome.answer),
        }))
    }
}

fn allows_secret(item: &Item, action: ActionId) -> bool {
    matches!(item.kind, Kind::SecretApproval | Kind::NodeJoin)
        && matches!(
            action,
            ActionId::Allow | ActionId::AllowOnce | ActionId::AllowAlways
        )
}

fn refused(item: &Item) -> InboxOutcome {
    InboxOutcome {
        answered: false,
        state: item.state,
        answer: item.answer,
    }
}

impl OpGroup for InboxOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(op, ops::INBOX_LIST | ops::INBOX_ANSWER | ops::INBOX_SEEN).then_some(OpSpec::APP)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App || caller.client != ClientKind::App {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "The inbox is the app's.",
            ));
        }
        match op {
            ops::INBOX_LIST => Ok(self.list()),
            ops::INBOX_ANSWER => self.answer(args),
            ops::INBOX_SEEN => self.seen(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::INBOX]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::INBOX).then_some(Tier::App)
    }
}
