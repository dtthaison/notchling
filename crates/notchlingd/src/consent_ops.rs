use crate::acting_ops::{now_ms, Room};
use crate::consent_events::{commit, emit};
use crate::consent_wire::view;
use crate::inbox_ops::{InboxOps, Responder};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use notchling_engine::consent::{
    clean_reason, item, spec, Consent, ConsentStore, New, Opened, Refusal, State, MAX_EXPIRY_MS,
    MIN_EXPIRY_MS,
};
use notchling_engine::inbox::{ActionId, Item, State as ItemState};
use notchling_events::kinds::choices::{Answerer, ConsentThen};
use notchling_events::scope::{self, ConsentNote};
use notchling_local_api::{
    bound_identity, ops, topics, AgentIdentity, ApiError, Caller, ClientKind, ErrorCode, OpSpec,
    Tier,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
#[cfg(test)]
use std::sync::OnceLock;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const CONSENT_OPS: [&str; 6] = [
    ops::CONSENT_GET,
    ops::CONSENT_MINE,
    ops::CONSENT_CANCEL,
    ops::CONSENT_LIST,
    ops::CONSENT_ANSWER,
    ops::CONSENT_OPENED,
];

pub const WAIT_MAX_MS: u64 = 50_000;
const TICK: Duration = Duration::from_secs(1);
const SLICE: Duration = Duration::from_millis(250);
const NO_SUCH: &str = "No such request.";
const OTHER_SESSION: &str = "Only the agent that asked can see this request.";
const LOG_DOWN: &str = "the audit log can't be written";

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
#[cfg(test)]
pub type Identify = Arc<dyn Fn(u32) -> AgentIdentity + Send + Sync>;

pub trait Handler: Send + Sync {
    fn recheck(&self, _row: &Consent) -> Result<(), String> {
        Ok(())
    }

    fn apply(
        &self,
        _row: &Consent,
        _by: Option<Answerer>,
        _device: Option<&str>,
    ) -> Result<Value, String> {
        Err("the app applies this one".to_string())
    }

    fn ended(&self, _row: &Consent, _then: ConsentThen, _device: Option<&str>) {}

    fn shape(&self, _row: &Consent, _view: &mut Value, _agent: bool) {}
}

pub struct Opening {
    pub kind: &'static str,
    pub target: String,
    pub change: Value,
    pub facts: Value,
    pub request: Value,
    pub reason: Option<String>,
    pub expires_in_ms: Option<i64>,
    pub key: Option<String>,
}

enum Msg {
    Settle(String),
    Stop,
}

pub struct ConsentOps {
    me: Weak<ConsentOps>,
    pub(crate) store: Arc<ConsentStore>,
    room: Arc<dyn Room>,
    inbox: Option<Arc<InboxOps>>,
    events: Option<Events>,
    #[cfg(test)]
    identify: OnceLock<Identify>,
    handlers: Mutex<HashMap<&'static str, Arc<dyn Handler>>>,
    clock: Clock,
    queue: Mutex<Option<Sender<Msg>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    gate: Mutex<()>,
    changed: Condvar,
    items: Mutex<HashMap<String, String>>,
    devices: Mutex<HashMap<String, String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn forbidden(message: &str, reason: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message).with_reason(reason)
}

pub(crate) fn options(args: &Value) -> (Option<String>, Option<i64>, Option<String>) {
    let consent = args.get("consent");
    let text = |name: &str| {
        consent
            .and_then(|c| c.get(name))
            .or_else(|| args.get(name).filter(|_| name == "reason"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let expires = consent
        .and_then(|c| c.get("expiresInMs"))
        .and_then(Value::as_i64);
    (text("reason"), expires, text("key"))
}

impl ConsentOps {
    pub fn new(
        store: Arc<ConsentStore>,
        room: Arc<dyn Room>,
        inbox: Option<Arc<InboxOps>>,
        events: Option<Events>,
    ) -> Arc<ConsentOps> {
        ConsentOps::with_clock(store, room, inbox, events, Arc::new(now_ms))
    }

    pub fn with_clock(
        store: Arc<ConsentStore>,
        room: Arc<dyn Room>,
        inbox: Option<Arc<InboxOps>>,
        events: Option<Events>,
        clock: Clock,
    ) -> Arc<ConsentOps> {
        Arc::new_cyclic(|me| ConsentOps {
            me: me.clone(),
            store,
            room,
            inbox,
            events,
            #[cfg(test)]
            identify: OnceLock::new(),
            handlers: Mutex::new(HashMap::new()),
            clock,
            queue: Mutex::new(None),
            thread: Mutex::new(None),
            gate: Mutex::new(()),
            changed: Condvar::new(),
            items: Mutex::new(HashMap::new()),
            devices: Mutex::new(HashMap::new()),
        })
    }

    pub fn for_daemon(
        data_dir: &Path,
        room: Arc<dyn Room>,
        inbox: Arc<InboxOps>,
        events: Events,
    ) -> Arc<ConsentOps> {
        ConsentOps::new(
            Arc::new(store_for(data_dir)),
            room,
            Some(inbox),
            Some(events),
        )
    }

    #[cfg(test)]
    pub fn bind_identity(&self, identify: Identify) {
        let _ = self.identify.set(identify);
    }

    pub fn register(&self, kind: &'static str, handler: Arc<dyn Handler>) {
        lock(&self.handlers).insert(kind, handler);
    }

    pub fn store(&self) -> &Arc<ConsentStore> {
        &self.store
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn who(&self, caller: &Caller) -> AgentIdentity {
        if let Some(bound) = bound_identity() {
            return bound;
        }
        #[cfg(test)]
        if let Some(identify) = self.identify.get() {
            return identify(caller.pid.unwrap_or(0));
        }
        let _ = caller;
        AgentIdentity::shell()
    }

    fn wake(&self) {
        let _gate = lock(&self.gate);
        self.changed.notify_all();
    }

    fn handler(&self, kind: &str) -> Option<Arc<dyn Handler>> {
        lock(&self.handlers).get(kind).cloned()
    }

    fn refusal(refused: Refusal) -> ApiError {
        match refused {
            Refusal::Busy { retry_after_ms } => failed(
                "Too many requests are waiting, or this one was just denied. Try again later.",
            )
            .with_data(json!({"reason": "busy", "retryAfterMs": retry_after_ms})),
            Refusal::LongExpiry => {
                ApiError::bad_request("That end time is longer than this caller may ask for.")
                    .with_reason("bad_value")
            }
            Refusal::KeyReused => {
                ApiError::bad_request("That key was used for a different request. Use a new key.")
                    .with_reason("key_reused")
            }
            Refusal::Db(_) => failed("Requests can't be saved right now."),
        }
    }

    fn expiry(kind: &str, asked: Option<i64>) -> Result<i64, ApiError> {
        let spec = spec(kind).ok_or_else(|| ApiError::internal("no such kind"))?;
        let ms = asked.unwrap_or(spec.default_expiry_ms);
        if !(MIN_EXPIRY_MS..=spec.max_expiry_ms.min(MAX_EXPIRY_MS)).contains(&ms) {
            return Err(
                ApiError::bad_request(notchling_engine::consent::rules::BAD_EXPIRY)
                    .with_reason("bad_value"),
            );
        }
        Ok(ms)
    }

    pub fn would(&self, o: &Opening) -> Result<Value, ApiError> {
        let spec = spec(o.kind).ok_or_else(|| ApiError::internal("no such kind"))?;
        let ms = Self::expiry(o.kind, o.expires_in_ms)?;
        Ok(json!({
            "kind": o.kind,
            "facts": o.facts,
            "answerOn": spec.answer_on.as_str(),
            "expiresInMs": ms,
        }))
    }

    pub fn open(&self, caller: &Caller, o: Opening) -> Result<Opened, ApiError> {
        let spec = spec(o.kind).ok_or_else(|| ApiError::internal("no such kind"))?;
        let expires_in_ms = Self::expiry(o.kind, o.expires_in_ms)?;
        let who = self.who(caller);
        let started = caller.pid.and_then(|pid| {
            crate::vault_caller::Processes::start_time(&crate::vault_caller::SystemProcesses, pid)
        });
        let new = New {
            kind: o.kind.to_string(),
            target: o.target,
            change: o.change,
            facts: o.facts,
            request: o.request,
            parts: None,
            reason: o.reason.map(|r| clean_reason(&r)).filter(|r| !r.is_empty()),
            session: who.session.clone().filter(|_| who.verified),
            tool: who.tool.name().to_string(),
            verified: who.verified,
            root: who.root.clone(),
            folder: who.folder.clone(),
            organization: who.organization.clone(),
            pid: caller.pid.unwrap_or(0),
            pid_start: started,
            idem_key: o
                .key
                .filter(|k| !k.is_empty())
                .map(|k| k.chars().take(64).collect()),
            answer_on: spec.answer_on,
            applied_by: spec.applied_by,
            expires_in_ms,
        };
        let opened = self.store.insert(&new, self.now()).map_err(Self::refusal)?;
        let row = opened.row().clone();
        self.note(&row);
        if let Opened::Fresh(_) = &opened {
            if !commit(self.events.as_ref(), &row, ConsentThen::Asked, self.now()) {
                let _ = self
                    .store
                    .finish(&row.id, State::Withdrawn, None, self.now());
                return Err(failed(LOG_DOWN));
            }
            self.raise(&row);
            self.tell(&row);
            self.wake();
        }
        Ok(opened)
    }

    pub fn note(&self, row: &Consent) {
        let note = if row.state.closed() {
            ConsentNote::Used
        } else {
            ConsentNote::Pending
        };
        scope::note_consent(&row.id, note);
    }

    fn raise(&self, row: &Consent) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let raised = item(row, self.now());
        let id = raised.id.clone();
        let responder = Answers {
            ops: self.me.clone(),
            id: row.id.clone(),
        };
        lock(&self.items).insert(row.id.clone(), id.clone());
        inbox.raise(raised, Some(Arc::new(responder)));
    }

    fn close_item(&self, row: &Consent, then: ConsentThen) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let Some(id) = lock(&self.items).remove(&row.id) else {
            return;
        };
        let by = row.by.as_deref();
        match then {
            ConsentThen::Applied | ConsentThen::Failed | ConsentThen::Allowed => {
                inbox.resolve(&id, ItemState::Answered, Some(ActionId::Allow), by)
            }
            ConsentThen::Denied => {
                inbox.resolve(&id, ItemState::Answered, Some(ActionId::Deny), by)
            }
            ConsentThen::Expired => inbox.resolve(&id, ItemState::Expired, None, None),
            ConsentThen::Withdrawn => inbox.withdraw(&id),
            _ => inbox.resolve(&id, ItemState::Withdrawn, None, None),
        }
    }

    pub fn decide(
        &self,
        id: &str,
        allow: bool,
        parts: Option<&Value>,
        by: &str,
        device: Option<String>,
    ) -> Result<Value, ApiError> {
        let row = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        if allow {
            if row.applied_by == notchling_engine::consent::AppliedBy::App {
                return Err(forbidden(
                    "Only the Mac's app applies this one, on its card.",
                    "app_only",
                ));
            }
            if !row.answer_on.may_allow(by) {
                return Err(forbidden("Only the Mac may allow this.", "app_only"));
            }
        }
        let to = if allow {
            State::Applying
        } else {
            State::Denied
        };
        let taken = self
            .store
            .answer(id, to, by, parts, self.now())
            .map_err(failed)?;
        match taken {
            notchling_engine::consent::Taken::Won(row) => {
                if let Some(device) = device {
                    lock(&self.devices).insert(row.id.clone(), device);
                }
                self.send(Msg::Settle(row.id.clone()));
                self.wake();
                Ok(json!({"answered": true, "state": row.state.as_str(), "by": by}))
            }
            notchling_engine::consent::Taken::Lost(row) => {
                Ok(json!({"answered": false, "state": row.state.as_str(), "by": row.by}))
            }
            notchling_engine::consent::Taken::Gone => Err(failed(NO_SUCH)),
        }
    }

    fn send(&self, msg: Msg) {
        if let Some(queue) = lock(&self.queue).as_ref() {
            let _ = queue.send(msg);
        }
    }

    fn device_of(&self, id: &str) -> Option<String> {
        lock(&self.devices).get(id).cloned()
    }

    fn closed(&self, row: &Consent, then: ConsentThen) {
        let device = lock(&self.devices).remove(&row.id);
        if let Some(handler) = self.handler(&row.kind) {
            handler.ended(row, then, device.as_deref());
        }
        if matches!(then, ConsentThen::Applied | ConsentThen::Cancelled) {
            commit(self.events.as_ref(), row, then, self.now());
        } else {
            emit(self.events.as_ref(), row, then, self.now());
        }
        self.close_item(row, then);
        self.tell(row);
        self.wake();
    }

    fn tell(&self, row: &Consent) {
        let Some(owner) = row.session.as_deref().filter(|_| row.in_session()) else {
            return;
        };
        let data = json!({"request": self.project(row, true), "state": row.state.as_str()});
        self.room
            .publish_to(owner, topics::MINE_CONSENTS, topics::CONSENT_CHANGED, &data);
    }

    fn settle(&self, id: &str) {
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        match row.state {
            State::Denied => self.closed(&row, ConsentThen::Denied),
            State::Applying => self.apply(row),
            _ => {}
        }
    }

    fn apply(&self, row: Consent) {
        let now = self.now();
        if now >= row.expires_ms {
            self.end(&row, State::Withdrawn, None, ConsentThen::Withdrawn);
            return;
        }
        if !commit(self.events.as_ref(), &row, ConsentThen::Allowed, now) {
            self.end(&row, State::Failed, None, ConsentThen::Failed);
            return;
        }
        let Some(handler) = self.handler(&row.kind) else {
            self.end(&row, State::Failed, None, ConsentThen::Failed);
            return;
        };
        if handler.recheck(&row).is_err() || self.now() >= row.expires_ms {
            self.end(&row, State::Withdrawn, None, ConsentThen::Withdrawn);
            return;
        }
        let by = row.by.as_deref().and_then(Answerer::from_name);
        let device = self.device_of(&row.id);
        match handler.apply(&row, by, device.as_deref()) {
            Ok(result) => self.end(&row, State::Applied, Some(&result), ConsentThen::Applied),
            Err(_) => self.end(&row, State::Failed, None, ConsentThen::Failed),
        }
    }

    fn end(&self, row: &Consent, to: State, result: Option<&Value>, then: ConsentThen) {
        let done = self
            .store
            .finish(&row.id, to, result, self.now())
            .ok()
            .flatten()
            .unwrap_or_else(|| row.clone());
        self.closed(&done, then);
    }

    pub fn use_for_app(
        &self,
        caller: &Caller,
        id: &str,
        kind: &str,
        target: &str,
    ) -> Result<Consent, ApiError> {
        Self::app_only(caller)?;
        let row = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        if row.kind != kind || row.target != target {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        if row.applied_by != notchling_engine::consent::AppliedBy::App {
            return Err(forbidden("The daemon applies this one.", "needs_consent"));
        }
        let op_id = scope::current()
            .op_id
            .map(|op| op.as_str().to_string())
            .unwrap_or_default();
        let used = self
            .store
            .used(id, &op_id, "mac", self.now())
            .map_err(failed)?
            .ok_or_else(|| {
                forbidden(
                    "That request is no longer waiting for your yes.",
                    "needs_consent",
                )
            })?;
        scope::note_consent(&used.id, ConsentNote::Used);
        Ok(used)
    }

    pub fn app_only(caller: &Caller) -> Result<(), ApiError> {
        if caller.tier == Tier::App && caller.client == ClientKind::App {
            Ok(())
        } else {
            Err(forbidden("Only the Notchling app may do this.", "app_only"))
        }
    }

    pub fn withdraw_use(&self, row: &Consent) {
        self.end(row, State::Withdrawn, None, ConsentThen::Withdrawn);
    }

    pub fn finish_use(&self, row: &Consent, outcome: Result<Value, String>) {
        match outcome {
            Ok(result) => self.end(row, State::Applied, Some(&result), ConsentThen::Applied),
            Err(_) => self.end(row, State::Failed, None, ConsentThen::Failed),
        }
    }

    pub fn deny_by_app(&self, id: &str) -> bool {
        self.decide(id, false, None, "mac", None)
            .is_ok_and(|v| v["answered"] == json!(true))
    }

    fn may_see(&self, caller: &Caller, row: &Consent) -> bool {
        if !row.verified {
            return true;
        }
        let who = self.who(caller);
        who.verified && who.session.is_some() && who.session == row.session
    }

    fn row_for(&self, caller: &Caller, args: &Value, usage: &str) -> Result<Consent, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage.to_string()))?;
        self.row_of(caller, id)
    }

    pub fn row_of(&self, caller: &Caller, id: &str) -> Result<Consent, ApiError> {
        let row = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        if Self::app_only(caller).is_err() && !self.may_see(caller, &row) {
            return Err(forbidden(OTHER_SESSION, "other_session"));
        }
        Ok(row)
    }

    pub fn wait_for(&self, id: &str, wait: Duration) -> Option<Consent> {
        let end = Instant::now() + wait;
        loop {
            let row = self.store.get(id).ok().flatten()?;
            let left = end.saturating_duration_since(Instant::now());
            if row.state.closed() || left.is_zero() {
                return Some(row);
            }
            let gate = lock(&self.gate);
            let _ = self
                .changed
                .wait_timeout(gate, left.min(SLICE))
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn get(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let row = self.row_for(caller, args, "consent.get takes {id, waitMs?}")?;
        let wait = args
            .get("waitMs")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(WAIT_MAX_MS);
        let row = if wait == 0 {
            row
        } else {
            self.wait_for(&row.id, Duration::from_millis(wait))
                .unwrap_or(row)
        };
        Ok(self.project(&row, Self::app_only(caller).is_err()))
    }

    pub fn project(&self, row: &Consent, agent: bool) -> Value {
        let mut out = view(row);
        if let Some(handler) = self.handler(&row.kind) {
            handler.shape(row, &mut out, agent);
        }
        out
    }

    fn mine(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.who(caller);
        let rows = match (who.verified, who.session) {
            (true, Some(session)) => self.store.of_session(&session).map_err(failed)?,
            _ => Vec::new(),
        };
        let states: Option<Vec<State>> = args.get("states").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().and_then(State::parse))
                .collect()
        });
        let rows: Vec<Value> = rows
            .iter()
            .filter(|r| states.as_ref().is_none_or(|s| s.contains(&r.state)))
            .map(|r| self.project(r, true))
            .collect();
        Ok(json!({"requests": rows}))
    }

    fn cancel(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let row = self.row_for(caller, args, "consent.cancel takes {id}")?;
        scope::note_consent(&row.id, ConsentNote::Used);
        let done = self
            .store
            .finish(&row.id, State::Cancelled, None, self.now())
            .map_err(failed)?;
        match done {
            Some(done) => {
                self.closed(&done, ConsentThen::Cancelled);
                Ok(json!({"cancelled": true, "state": done.state.as_str()}))
            }
            None => Ok(json!({"cancelled": false, "state": row.state.as_str()})),
        }
    }

    fn list(&self, caller: &Caller) -> Result<Value, ApiError> {
        Self::app_only(caller)?;
        let mut rows = self.store.all().map_err(failed)?;
        rows.sort_by_key(|r| (r.state.closed(), -r.created_ms));
        let rows: Vec<Value> = rows
            .iter()
            .take(200)
            .map(|r| self.project(r, false))
            .collect();
        Ok(json!({"requests": rows}))
    }

    fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::app_only(caller)?;
        let usage = "consent.answer takes {consentId, decision: \"allow\" | \"deny\", parts?}";
        let id = args
            .get("consentId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let allow = match args.get("decision").and_then(Value::as_str) {
            Some("allow") => true,
            Some("deny") => false,
            _ => return Err(ApiError::bad_request(usage)),
        };
        self.decide(id, allow, args.get("parts"), "mac", None)
    }

    fn opened(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::app_only(caller)?;
        let id = args
            .get("consentId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("consent.opened takes {consentId}"))?;
        let was = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?
            .state;
        let row = self
            .store
            .open_sheet(id, self.now())
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        if row.state == State::Opened && was == State::Waiting {
            emit(self.events.as_ref(), &row, ConsentThen::Opened, self.now());
            self.tell(&row);
            self.wake();
        }
        Ok(json!({"state": row.state.as_str()}))
    }

    pub fn load(&self) {
        let now = self.now();
        let Ok(restart) = self.store.restart(now) else {
            return;
        };
        for row in &restart.withdrawn {
            self.closed(row, ConsentThen::Withdrawn);
        }
        for row in &restart.expired {
            self.closed(row, ConsentThen::Expired);
        }
        for row in &restart.waiting {
            emit(self.events.as_ref(), row, ConsentThen::Restored, now);
            self.raise(row);
        }
        self.wake();
    }

    pub fn tick(&self) {
        if let Ok(due) = self.store.expire_due(self.now()) {
            for row in &due {
                self.closed(row, ConsentThen::Expired);
            }
        }
    }

    pub fn start(&self) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let (tx, rx) = channel::<Msg>();
        *lock(&self.queue) = Some(tx);
        let me = self.me.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-consent".into())
            .spawn(move || loop {
                let msg = rx.recv_timeout(TICK);
                let Some(ops) = me.upgrade() else {
                    return;
                };
                match msg {
                    Ok(Msg::Settle(id)) => ops.settle(&id),
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => ops.tick(),
                }
            })
            .ok();
    }

    pub fn close(&self) {
        self.send(Msg::Stop);
        let queue = lock(&self.queue).take();
        drop(queue);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }

    pub fn waiting_for(&self, kind: &str, target: &str) -> Vec<Consent> {
        self.store
            .open_rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.kind == kind && r.target == target && r.state.open())
            .collect()
    }

    pub fn pending_for(&self, caller: &Caller, kind: &str, target: &str) -> Vec<Consent> {
        let rows = self.waiting_for(kind, target);
        if Self::app_only(caller).is_ok() {
            return rows;
        }
        let who = self.who(caller);
        rows.into_iter()
            .filter(|r| {
                r.verified && who.verified && who.session.is_some() && who.session == r.session
            })
            .collect()
    }

    pub fn app_attached(&self) -> bool {
        self.room.can_answer()
    }
}

pub fn store_for(data_dir: &Path) -> ConsentStore {
    ConsentStore::open(data_dir).unwrap_or_else(|_| ConsentStore::unavailable())
}

struct Answers {
    ops: Weak<ConsentOps>,
    id: String,
}

impl Answers {
    fn give(&self, action: ActionId, by: &str, device: Option<String>) -> bool {
        let allow = match action {
            ActionId::Allow | ActionId::AllowOnce => true,
            ActionId::Deny => false,
            _ => return false,
        };
        let Some(ops) = self.ops.upgrade() else {
            return false;
        };
        ops.decide(&self.id, allow, None, by, device)
            .is_ok_and(|v| v["answered"] == json!(true))
    }
}

impl Responder for Answers {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        self.give(action, "mac", None)
    }

    fn answered_by(&self, _item: &Item, action: ActionId, by: &str) -> bool {
        let device = scope::current()
            .actor
            .and_then(|a| a.device)
            .map(|d| d.as_str().to_owned());
        self.give(action, by, device)
    }
}

impl OpGroup for ConsentOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::CONSENT_GET | ops::CONSENT_MINE => Some(OpSpec::AGENT_READ),
            ops::CONSENT_CANCEL => Some(OpSpec::AGENT_ACTS),
            ops::CONSENT_LIST => Some(OpSpec::APP_QUIET),
            ops::CONSENT_ANSWER | ops::CONSENT_OPENED => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CONSENT_GET => self.get(caller, args),
            ops::CONSENT_MINE => self.mine(caller, args),
            ops::CONSENT_CANCEL => self.cancel(caller, args),
            ops::CONSENT_LIST => self.list(caller),
            ops::CONSENT_ANSWER => self.answer(caller, args),
            ops::CONSENT_OPENED => self.opened(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "consent_ops_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "consent_ops_review_tests.rs"]
mod review_tests;

#[cfg(test)]
#[path = "consent_scoped_tests.rs"]
mod scoped_tests;
