use crate::inbox_ops::{InboxOps, Responder};
use notchling_engine::inbox::{ActionId, Item, State};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const ASK_WAIT: Duration = Duration::from_secs(300);
pub const MAX_ASKS: usize = 8;
pub const KEEP_DONE: Duration = Duration::from_secs(600);
const TICK: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskState {
    Waiting,
    Allowed,
    Applied,
    Denied,
    Expired,
    Withdrawn,
    Failed,
}

impl AskState {
    pub fn as_str(self) -> &'static str {
        match self {
            AskState::Waiting => "waiting",
            AskState::Allowed => "applying",
            AskState::Applied => "applied",
            AskState::Denied => "denied",
            AskState::Expired => "expired",
            AskState::Withdrawn => "withdrawn",
            AskState::Failed => "failed",
        }
    }

    pub fn ended(self) -> bool {
        !matches!(self, AskState::Waiting | AskState::Allowed)
    }
}

#[derive(Clone, Debug)]
pub struct Ask {
    pub key: &'static str,
    pub value: Value,
    pub state: AskState,
    pub message: Option<String>,
    pub peer: Option<PathBuf>,
    deadline: Instant,
    ended: Option<Instant>,
}

impl Ask {
    pub fn new(key: &'static str, value: Value, peer: Option<PathBuf>, wait: Duration) -> Ask {
        Ask {
            key,
            value,
            state: AskState::Waiting,
            message: None,
            peer,
            deadline: Instant::now() + wait,
            ended: None,
        }
    }
}

pub fn item_id(id: &str) -> String {
    format!("setting:{id}")
}

pub fn view(id: &str, ask: &Ask) -> Value {
    let message = ask.message.clone().unwrap_or_else(|| {
        match ask.state {
            AskState::Waiting => "Waiting for the user to allow it in Notchling's Inbox.",
            AskState::Allowed => "Allowed; applying it.",
            AskState::Applied => "The user allowed it, and it's set.",
            AskState::Denied => "The user denied it.",
            AskState::Expired => "The user didn't answer in time.",
            AskState::Withdrawn => "Notchling stopped before the user answered.",
            AskState::Failed => "It couldn't be set.",
        }
        .to_string()
    });
    json!({"id": id, "key": ask.key, "value": ask.value, "state": ask.state.as_str(), "message": message})
}

pub enum Msg {
    Allowed(String),
    Denied(String),
}

pub trait Applier: Send + Sync {
    fn apply(&self, id: &str, ask: &Ask) -> Result<(), String>;
    fn ended(&self, id: &str, ask: &Ask);
}

pub struct Asks {
    map: Mutex<HashMap<String, Ask>>,
    changed: Condvar,
    queue: Mutex<Option<Sender<Msg>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Asks {
    pub fn new() -> Arc<Asks> {
        Arc::new(Asks {
            map: Mutex::new(HashMap::new()),
            changed: Condvar::new(),
            queue: Mutex::new(None),
            thread: Mutex::new(None),
        })
    }

    pub fn open(&self, id: &str, ask: Ask) -> bool {
        let mut map = lock(&self.map);
        if map.values().filter(|a| !a.state.ended()).count() >= MAX_ASKS {
            return false;
        }
        map.insert(id.to_string(), ask);
        true
    }

    pub fn get(&self, id: &str) -> Option<Ask> {
        lock(&self.map).get(id).cloned()
    }

    pub fn waiting_for(&self, key: &str) -> Vec<(String, Ask)> {
        let mut found: Vec<(String, Ask)> = lock(&self.map)
            .iter()
            .filter(|(_, a)| a.key == key && !a.state.ended())
            .map(|(id, a)| (id.clone(), a.clone()))
            .collect();
        found.sort_by_key(|(_, a)| a.deadline);
        found
    }

    pub fn wait(&self, id: &str, within: Duration) -> Option<Ask> {
        let end = Instant::now() + within;
        let mut map = lock(&self.map);
        loop {
            let ask = map.get(id)?;
            let left = end.saturating_duration_since(Instant::now());
            if ask.state.ended() || left.is_zero() {
                return Some(ask.clone());
            }
            map = self
                .changed
                .wait_timeout(map, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn answer(&self, id: &str, action: ActionId) -> bool {
        let msg = {
            let mut map = lock(&self.map);
            let Some(ask) = map.get_mut(id) else {
                return false;
            };
            if ask.state != AskState::Waiting {
                return false;
            }
            match action {
                ActionId::Allow | ActionId::AllowOnce | ActionId::AllowAlways => {
                    ask.state = AskState::Allowed;
                    Msg::Allowed(id.to_string())
                }
                ActionId::Deny => {
                    ask.state = AskState::Denied;
                    ask.ended = Some(Instant::now());
                    Msg::Denied(id.to_string())
                }
                _ => return false,
            }
        };
        self.changed.notify_all();
        if let Some(queue) = lock(&self.queue).as_ref() {
            let _ = queue.send(msg);
        }
        true
    }

    fn finish(&self, id: &str, state: AskState, message: Option<String>) -> Option<Ask> {
        let done = {
            let mut map = lock(&self.map);
            let ask = map.get_mut(id)?;
            ask.state = state;
            ask.message = message;
            ask.ended = Some(Instant::now());
            ask.clone()
        };
        self.changed.notify_all();
        Some(done)
    }

    fn take_overdue(&self, now: Instant) -> Vec<(String, Ask)> {
        let mut map = lock(&self.map);
        let mut overdue = Vec::new();
        for (id, ask) in map.iter_mut() {
            if ask.state == AskState::Waiting && now >= ask.deadline {
                ask.state = AskState::Expired;
                ask.ended = Some(now);
                overdue.push((id.clone(), ask.clone()));
            }
        }
        map.retain(|_, a| a.ended.is_none_or(|t| now.duration_since(t) < KEEP_DONE));
        drop(map);
        if !overdue.is_empty() {
            self.changed.notify_all();
        }
        overdue
    }

    fn take_waiting(&self) -> Vec<(String, Ask)> {
        let mut map = lock(&self.map);
        let now = Instant::now();
        let mut left = Vec::new();
        for (id, ask) in map.iter_mut() {
            if ask.state == AskState::Waiting {
                ask.state = AskState::Withdrawn;
                ask.ended = Some(now);
                left.push((id.clone(), ask.clone()));
            }
        }
        drop(map);
        self.changed.notify_all();
        left
    }

    pub fn responder(self: &Arc<Self>, id: &str) -> Arc<dyn Responder> {
        Arc::new(AskResponder {
            asks: self.clone(),
            id: id.to_string(),
        })
    }

    pub fn start(self: &Arc<Self>, applier: Arc<dyn Applier>, inbox: Option<Arc<InboxOps>>) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let (tx, rx) = channel::<Msg>();
        *lock(&self.queue) = Some(tx);
        let asks = self.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-settings-asks".into())
            .spawn(move || run(&asks, rx, applier.as_ref(), inbox.as_deref()))
            .ok();
    }

    pub fn close(&self) {
        let queue = lock(&self.queue).take();
        drop(queue);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }
}

struct AskResponder {
    asks: Arc<Asks>,
    id: String,
}

impl Responder for AskResponder {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.asks.answer(&self.id, action)
    }
}

fn close_item(inbox: Option<&InboxOps>, id: &str, state: State) {
    if let Some(inbox) = inbox {
        inbox.resolve(&item_id(id), state, None, None);
    }
}

fn run(asks: &Asks, rx: Receiver<Msg>, applier: &dyn Applier, inbox: Option<&InboxOps>) {
    loop {
        match rx.recv_timeout(TICK) {
            Ok(Msg::Allowed(id)) => {
                let Some(ask) = asks.get(&id) else {
                    continue;
                };
                let done = match applier.apply(&id, &ask) {
                    Ok(()) => asks.finish(&id, AskState::Applied, None),
                    Err(why) => asks.finish(&id, AskState::Failed, Some(why)),
                };
                if let Some(done) = done {
                    applier.ended(&id, &done);
                }
            }
            Ok(Msg::Denied(id)) => {
                if let Some(ask) = asks.get(&id) {
                    applier.ended(&id, &ask);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                for (id, ask) in asks.take_overdue(Instant::now()) {
                    close_item(inbox, &id, State::Expired);
                    applier.ended(&id, &ask);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                for (id, ask) in asks.take_waiting() {
                    close_item(inbox, &id, State::Withdrawn);
                    applier.ended(&id, &ask);
                }
                return;
            }
        }
    }
}
