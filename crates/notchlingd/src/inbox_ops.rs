use crate::acting_ops::{now_ms, Room};
use crate::op_group::OpGroup;
use notchling_engine::inbox::{self, ActionId, AnswerError, Book, Change, Item, State};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(1);
const ANSWERED_BY: &str = "mac";

pub trait Responder: Send + Sync {
    fn answered(&self, item: &Item, action: ActionId);
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

struct Inner {
    book: Book,
    responders: HashMap<String, Arc<dyn Responder>>,
}

impl Inner {
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
    data_dir: PathBuf,
    room: Arc<dyn Room>,
    clock: Clock,
    inner: Mutex<Inner>,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
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
        let now = clock();
        let book = inbox::load(&data_dir, now).unwrap_or_else(|why| {
            eprintln!("notchling: reading the inbox failed ({why}); starting it empty");
            Book::new()
        });
        let ops = InboxOps {
            data_dir,
            room,
            clock,
            inner: Mutex::new(Inner {
                book,
                responders: HashMap::new(),
            }),
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        };
        let withdrawn: Vec<Change> = lock(&ops.inner)
            .book
            .items()
            .into_iter()
            .filter(|item| item.state == State::Withdrawn && item.closed_ms == Some(now))
            .map(Change::Updated)
            .collect();
        ops.persist(&withdrawn);
        Arc::new(ops)
    }

    pub fn raise(&self, item: Item, responder: Option<Arc<dyn Responder>>) -> String {
        let id = item.id.clone();
        let changes = {
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
            self.persist(&changes);
            changes
        };
        self.publish(&changes);
        id
    }

    pub fn resolve(&self, id: &str, state: State, answer: Option<ActionId>) {
        let now = (self.clock)();
        let changes: Vec<Change> = {
            let mut inner = lock(&self.inner);
            let changes: Vec<Change> = inner
                .book
                .resolve(id, state, answer, now)
                .into_iter()
                .collect();
            inner.responders.remove(id);
            self.persist(&changes);
            changes
        };
        self.publish(&changes);
    }

    pub fn tick(&self, now_ms: i64) {
        let changes = {
            let mut inner = lock(&self.inner);
            let mut changes = inner.book.expire(now_ms);
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
            self.persist(&changes);
            changes
        };
        self.publish(&changes);
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

    pub fn close(&self) {
        let stop = lock(&self.stop).take();
        drop(stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }

    fn persist(&self, changes: &[Change]) {
        for change in changes {
            if let Err(why) = inbox::save(&self.data_dir, change) {
                eprintln!("notchling: saving the inbox failed ({why})");
            }
        }
    }

    fn publish(&self, changes: &[Change]) {
        for change in changes {
            match change {
                Change::Raised(item) | Change::Updated(item) => {
                    self.room
                        .publish(topics::INBOX, topics::INBOX_CHANGED, &json!({"item": item}));
                }
                Change::Removed(id) => {
                    self.room
                        .publish(topics::INBOX, topics::INBOX_REMOVED, &json!({"id": id}));
                }
            }
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
        let (changes, unread) = {
            let mut inner = lock(&self.inner);
            let changes = inner.book.mark_seen(&ids);
            self.persist(&changes);
            (changes, inner.book.unread())
        };
        self.publish(&changes);
        Ok(json!({"unread": unread}))
    }

    fn answer(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("id is required"))?;
        let action: ActionId =
            serde_json::from_value(args.get("action").cloned().unwrap_or(Value::Null))
                .map_err(|_| bad("action is not one the inbox knows"))?;
        let now = (self.clock)();
        let (change, responder) = {
            let mut inner = lock(&self.inner);
            match inner.book.answer(id, action, ANSWERED_BY, now) {
                Ok(change) => {
                    self.persist(std::slice::from_ref(&change));
                    (change, inner.responders.remove(id))
                }
                Err(AnswerError::NotOpen(state)) => {
                    let answer = inner
                        .book
                        .items()
                        .into_iter()
                        .find(|item| item.id == id)
                        .and_then(|item| item.answer);
                    return Ok(
                        json!({"answered": false, "state": state_name(state), "answer": answer_json(answer)}),
                    );
                }
                Err(AnswerError::Unknown) => {
                    return Err(ApiError::new(ErrorCode::Failed, "No such inbox item."));
                }
                Err(AnswerError::NotAnAction) => {
                    return Err(bad("The item doesn't offer that action."));
                }
            }
        };
        self.publish(std::slice::from_ref(&change));
        if let (Change::Updated(item), Some(responder)) = (&change, responder) {
            responder.answered(item, action);
        }
        Ok(
            json!({"answered": true, "state": state_name(State::Answered), "answer": answer_json(Some(action))}),
        )
    }
}

impl OpGroup for InboxOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(op, ops::INBOX_LIST | ops::INBOX_ANSWER | ops::INBOX_SEEN).then_some(OpSpec::APP)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App {
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
