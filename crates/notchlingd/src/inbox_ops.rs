use crate::acting_ops::{now_ms, Room};
use crate::op_group::OpGroup;
use notchling_engine::bridge::inbox::InboxOutcome;
use notchling_engine::inbox::{self, ActionId, Book, Change, Item, Kind, State};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(1);
const ANSWERED_BY: &str = "mac";
const MAY_ALLOW: [&str; 2] = ["mac", "phone"];

pub trait Responder: Send + Sync {
    fn answered(&self, item: &Item, action: ActionId) -> bool;
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

enum Msg {
    Changes(Vec<Change>),
    Flush(Sender<()>),
}

struct Inner {
    book: Book,
    responders: HashMap<String, Arc<dyn Responder>>,
    writes: Option<Sender<Msg>>,
}

impl Inner {
    fn send(&self, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        if let Some(writes) = &self.writes {
            let _ = writes.send(Msg::Changes(changes));
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
    loaded: AtomicBool,
    inner: Mutex<Inner>,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

fn write_loop(reads: Receiver<Msg>, data_dir: PathBuf, room: Arc<dyn Room>) {
    while let Ok(msg) = reads.recv() {
        match msg {
            Msg::Changes(changes) => {
                for change in &changes {
                    persist(&data_dir, change);
                    publish(room.as_ref(), change);
                }
            }
            Msg::Flush(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

fn persist(data_dir: &Path, change: &Change) {
    if let Err(why) = inbox::save(data_dir, change) {
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
        let writer = std::thread::Builder::new()
            .name("notchling-inbox-writes".into())
            .spawn(move || write_loop(reads, folder, room))
            .ok();
        let inner = Inner {
            book: Book::new(),
            responders: HashMap::new(),
            writes: Some(writes),
        };
        let ops = InboxOps {
            clock,
            data_dir,
            loaded: AtomicBool::new(false),
            inner: Mutex::new(inner),
            stop: Mutex::new(None),
            thread: Mutex::new(None),
            writer: Mutex::new(writer),
        };
        Arc::new(ops)
    }

    pub fn start(self: &Arc<Self>) {
        self.load_saved();
        self.spawn_loop();
    }

    fn load_saved(&self) {
        if self.loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let now = (self.clock)();
        let saved = inbox::load(&self.data_dir, now).unwrap_or_else(|why| {
            eprintln!("notchling: reading the inbox failed ({why}); starting it empty");
            Book::new()
        });
        let mut inner = lock(&self.inner);
        let held = inner.book.items();
        let fresh: Vec<Item> = saved
            .items()
            .into_iter()
            .filter(|item| !held.iter().any(|h| h.id == item.id))
            .collect();
        let withdrawn: Vec<Change> = fresh
            .iter()
            .filter(|item| item.state == State::Withdrawn && item.closed_ms == Some(now))
            .cloned()
            .map(Change::Updated)
            .collect();
        inner.book = Book::from_items(fresh.into_iter().chain(held).collect());
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
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "Only the Mac or a paired phone may allow a secret.",
            ));
        }
        if let Some(responder) = inner.responders.get(id).cloned() {
            if !responder.answered(&current, action) {
                return Ok(refused(&current));
            }
        }
        match inner.book.answer(id, action, by, now) {
            Ok(change) => {
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
    item.kind == Kind::SecretApproval
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
