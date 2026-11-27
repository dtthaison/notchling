use crate::acting_ops::{now_ms, Room};
use crate::consent_answers::Answers;
use crate::consent_apart::Applies;
use crate::consent_events::{commit, emit};
use crate::consent_failure::failure_reason;
pub use crate::consent_failure::Failure;
use crate::inbox_ops::{Closing, InboxOps};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::presence::{Presence, GRACE_MS};
use notchling_engine::consent::{
    item, spec, Consent, ConsentStore, Opened, Refusal, State, MAX_EXPIRY_MS, MIN_EXPIRY_MS,
};
use notchling_engine::inbox::{self, Action, ActionId, OutcomeState, State as ItemState};
use notchling_events::kinds::choices::{Answerer, ConsentThen};
use notchling_events::scope::{self, ConsentNote};
use notchling_local_api::{
    bound_identity, ops, topics, AgentIdentity, ApiError, Caller, ClientKind, ErrorCode, OpSpec,
    Tier,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
#[cfg(test)]
use std::sync::OnceLock;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

pub const CONSENT_OPS: [&str; 6] = [
    ops::CONSENT_GET,
    ops::CONSENT_MINE,
    ops::CONSENT_CANCEL,
    ops::CONSENT_LIST,
    ops::CONSENT_ANSWER,
    ops::CONSENT_OPENED,
];

pub const WAIT_MAX_MS: u64 = 50_000;
pub const HELD_MS: i64 = 5 * 60_000;
const TICK: Duration = Duration::from_secs(1);
pub(crate) const SLICE: Duration = Duration::from_millis(250);
pub(crate) const NO_SUCH: &str = "No such request.";
pub(crate) const OTHER_SESSION: &str = "Only the agent that asked can see this request.";
const LOG_DOWN: &str = "the audit log can't be written";

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
#[cfg(test)]
pub type Identify = Arc<dyn Fn(u32) -> AgentIdentity + Send + Sync>;

pub trait Handler: Send + Sync {
    fn recheck(&self, _row: &Consent) -> Result<(), String> {
        Ok(())
    }

    fn apply_failing(
        &self,
        row: &Consent,
        by: Option<Answerer>,
        device: Option<&str>,
    ) -> Result<Value, Failure> {
        self.apply(row, by, device).map_err(|_| Failure::unspoken())
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

    fn forwarded(&self, _row: &Consent) -> bool {
        false
    }

    fn landed(&self, _row: &Consent) -> Option<Value> {
        None
    }

    fn handover(&self, _row: &Consent, _who: &AgentIdentity) -> Option<Value> {
        None
    }

    fn tick(&self, _now_ms: i64) {}

    fn slow(&self, _row: &Consent) -> bool {
        false
    }
}

pub struct Opening {
    pub kind: &'static str,
    pub target: String,
    pub change: Value,
    pub facts: Value,
    pub request: Value,
    pub parts: Option<Value>,
    pub reason: Option<String>,
    pub expires_in_ms: Option<i64>,
    pub key: Option<String>,
    pub retry_of: Option<String>,
}

#[derive(Clone)]
pub(crate) enum Msg {
    Settle(String),
    Withdrawn(String),
    Stop,
}

pub struct ConsentOps {
    pub(crate) me: Weak<ConsentOps>,
    pub(crate) store: Arc<ConsentStore>,
    room: Arc<dyn Room>,
    pub(crate) inbox: Option<Arc<InboxOps>>,
    events: Option<Events>,
    #[cfg(test)]
    identify: OnceLock<Identify>,
    handlers: Mutex<HashMap<&'static str, Arc<dyn Handler>>>,
    clock: Clock,
    pub(crate) queue: Mutex<Option<Sender<Msg>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    pub(crate) applies: Applies,
    pub(crate) gate: Mutex<()>,
    pub(crate) changed: Condvar,
    pub(crate) items: Mutex<HashMap<String, String>>,
    devices: Mutex<HashMap<String, String>>,
    pub(crate) presence: Arc<Presence>,
    pub(crate) restored: Mutex<HashSet<String>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

pub(crate) fn forbidden(message: &str, reason: &str) -> ApiError {
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

pub(crate) fn retry_of(args: &Value) -> Option<String> {
    args.get("consent")
        .and_then(|c| c.get("retryOf"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
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
        let presence = Presence::new(GRACE_MS, clock.clone());
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
            applies: Applies::default(),
            gate: Mutex::new(()),
            changed: Condvar::new(),
            items: Mutex::new(HashMap::new()),
            devices: Mutex::new(HashMap::new()),
            presence,
            restored: Mutex::new(HashSet::new()),
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

    pub(crate) fn handler(&self, kind: &str) -> Option<Arc<dyn Handler>> {
        lock(&self.handlers).get(kind).cloned()
    }

    pub(crate) fn refusal(refused: Refusal) -> ApiError {
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
            Refusal::RetryRefused(why) => forbidden(why, "other"),
            Refusal::Db(_) => failed("Requests can't be saved right now."),
        }
    }

    pub(crate) fn expiry(kind: &str, asked: Option<i64>) -> Result<i64, ApiError> {
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
        crate::consent_parts::check_opening(o.kind, o.parts.as_ref())?;
        let retry = o.retry_of.clone();
        let mut new = self.build(caller, o)?;
        if let Some(old) = retry {
            new.change = self.retried(&old, &new)?;
        }
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
        let mut raised = item(row, self.now());
        if self.handler(&row.kind).is_some_and(|h| h.forwarded(row))
            && !raised.actions.iter().any(|a| a.id == ActionId::Allow)
        {
            for offered in &mut raised.actions {
                offered.primary = false;
            }
            raised.actions.insert(
                0,
                Action {
                    id: ActionId::Allow,
                    label: "Allow".to_string(),
                    primary: true,
                },
            );
        }
        let id = raised.id.clone();
        let responder = Answers {
            ops: self.me.clone(),
            id: row.id.clone(),
        };
        lock(&self.items).insert(row.id.clone(), id.clone());
        inbox.raise(raised, Some(Arc::new(responder)));
        self.show_live(&row.id);
    }

    fn close_item(&self, row: &Consent, then: ConsentThen) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        lock(&self.restored).remove(&row.id);
        let Some(id) = lock(&self.items).remove(&row.id) else {
            return;
        };
        let by = row.by.as_deref();
        let now = self.now();
        let outcome = |state, reason: Option<&str>| Some(inbox::outcome(state, reason, now));
        let answered = |answer| Closing::Resolve {
            state: ItemState::Answered,
            answer: Some(answer),
            by,
        };
        match then {
            ConsentThen::Applied => inbox.close_item(
                &id,
                answered(ActionId::Allow),
                outcome(OutcomeState::Applied, None),
            ),
            ConsentThen::Failed => {
                let reason = failure_reason(row);
                inbox.close_item(
                    &id,
                    answered(ActionId::Allow),
                    outcome(OutcomeState::Failed, reason.as_deref()),
                )
            }
            ConsentThen::Allowed => inbox.close_item(
                &id,
                answered(ActionId::Allow),
                outcome(OutcomeState::Applying, None),
            ),
            ConsentThen::Denied => {
                if let Some(note) = &row.note {
                    inbox.note_item(&id, note);
                }
                inbox.close_item(&id, answered(ActionId::Deny), None)
            }
            ConsentThen::Expired => inbox.close_item(
                &id,
                Closing::Resolve {
                    state: ItemState::Expired,
                    answer: None,
                    by: None,
                },
                None,
            ),
            ConsentThen::Withdrawn => inbox.close_item(
                &id,
                Closing::Withdraw,
                outcome(OutcomeState::Withdrawn, None).filter(|_| row.by.is_some()),
            ),
            _ => inbox.close_item(
                &id,
                Closing::Resolve {
                    state: ItemState::Withdrawn,
                    answer: None,
                    by: None,
                },
                None,
            ),
        }
    }

    fn applying(&self, row: &Consent) {
        let (Some(inbox), Some(id)) = (&self.inbox, lock(&self.items).get(&row.id).cloned()) else {
            return;
        };
        inbox.set_outcome(
            &id,
            inbox::outcome(OutcomeState::Applying, None, self.now()),
        );
    }

    pub fn decide(
        &self,
        id: &str,
        allow: bool,
        parts: Option<&Value>,
        by: &str,
        device: Option<String>,
    ) -> Result<Value, ApiError> {
        self.decide_noted(id, allow, (parts, None), by, device)
    }

    pub fn decide_noted(
        &self,
        id: &str,
        allow: bool,
        (parts, note): (Option<&Value>, Option<&str>),
        by: &str,
        device: Option<String>,
    ) -> Result<Value, ApiError> {
        if allow && note.is_some() {
            return Err(ApiError::bad_request("A note goes with a no only."));
        }
        let row = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        let kept = crate::consent_parts::vet(&row, allow, parts)?;
        if allow {
            if row.applied_by == notchling_engine::consent::AppliedBy::App
                && !self.handler(&row.kind).is_some_and(|h| h.forwarded(&row))
            {
                return Err(forbidden(
                    "Only the Mac's app applies this one, on its card.",
                    "app_only",
                ));
            }
            if !row.answer_on.may_allow(by) {
                return Err(forbidden("Only the Mac may allow this.", "app_only"));
            }
        }
        let to = if allow && self.stale_at_allow(&row) {
            State::Withdrawn
        } else if allow {
            State::Applying
        } else {
            State::Denied
        };
        let taken = self
            .store
            .answer_noted(id, to, by, kept.as_ref(), note, self.now())
            .map_err(failed)?;
        match taken {
            notchling_engine::consent::Taken::Won(row) if row.state == State::Withdrawn => {
                self.deliver(Msg::Withdrawn(row.id.clone()));
                self.wake();
                Ok(json!({"answered": false, "state": row.state.as_str(), "by": by}))
            }
            notchling_engine::consent::Taken::Won(row) => {
                if let Some(device) = device {
                    lock(&self.devices).insert(row.id.clone(), device);
                }
                self.deliver(Msg::Settle(row.id.clone()));
                self.wake();
                Ok(json!({"answered": true, "state": row.state.as_str(), "by": by}))
            }
            notchling_engine::consent::Taken::Lost(row) => {
                Ok(json!({"answered": false, "state": row.state.as_str(), "by": row.by}))
            }
            notchling_engine::consent::Taken::Gone => Err(failed(NO_SUCH)),
        }
    }

    fn device_of(&self, id: &str) -> Option<String> {
        lock(&self.devices).get(id).cloned()
    }

    pub(crate) fn closed(&self, row: &Consent, then: ConsentThen) {
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

    pub(crate) fn settle(&self, id: &str) {
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        match row.state {
            State::Denied => self.closed(&row, ConsentThen::Denied),
            State::Applying if self.handler(&row.kind).is_some_and(|h| h.slow(&row)) => {
                self.applying(&row);
                self.apply_apart(row)
            }
            State::Applying => {
                self.applying(&row);
                self.apply(row)
            }
            _ => {}
        }
    }

    pub(crate) fn withdrawn_at_allow(&self, id: &str) {
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        if row.state == State::Withdrawn {
            self.closed(&row, ConsentThen::Withdrawn);
        }
    }

    fn stale_at_allow(&self, row: &Consent) -> bool {
        row.applied_by == notchling_engine::consent::AppliedBy::App
            && matches!(row.state, State::Waiting | State::Opened)
            && self
                .handler(&row.kind)
                .is_some_and(|handler| handler.recheck(row).is_err())
    }

    pub(crate) fn apply(&self, row: Consent) {
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
        if row.applied_by == notchling_engine::consent::AppliedBy::App {
            if let Some(result) = handler.landed(&row) {
                self.end(&row, State::Applied, Some(&result), ConsentThen::Applied);
            }
            return;
        }
        if handler.recheck(&row).is_err() || self.now() >= row.expires_ms {
            self.end(&row, State::Withdrawn, None, ConsentThen::Withdrawn);
            return;
        }
        let by = row.by.as_deref().and_then(Answerer::from_name);
        let device = self.device_of(&row.id);
        match handler.apply_failing(&row, by, device.as_deref()) {
            Ok(result) => self.end(&row, State::Applied, Some(&result), ConsentThen::Applied),
            Err(failure) => self.end_failed(&row, &failure),
        }
    }

    pub(crate) fn end_failed(&self, row: &Consent, failure: &Failure) {
        self.end(
            row,
            State::Failed,
            failure.stored().as_ref(),
            ConsentThen::Failed,
        );
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
        self.take_for_app(caller, id, kind, Some(target))
    }

    pub fn take_for_app(
        &self,
        caller: &Caller,
        id: &str,
        kind: &str,
        target: Option<&str>,
    ) -> Result<Consent, ApiError> {
        self.take_for_app_keeping(caller, id, kind, target, None)
    }

    pub fn take_for_app_keeping(
        &self,
        caller: &Caller,
        id: &str,
        kind: &str,
        target: Option<&str>,
        parts: Option<&Value>,
    ) -> Result<Consent, ApiError> {
        Self::app_only(caller)?;
        let row = self.get_row(id)?;
        if row.kind != kind || target.is_some_and(|target| row.target != target) {
            return Err(forbidden(
                "That request is for something else.",
                "needs_consent",
            ));
        }
        if row.applied_by != notchling_engine::consent::AppliedBy::App {
            return Err(forbidden("The daemon applies this one.", "needs_consent"));
        }
        let kept = crate::consent_parts::vet(&row, true, parts)?;
        let op_id = scope::current()
            .op_id
            .map(|op| op.as_str().to_string())
            .unwrap_or_default();
        let used = self
            .store
            .used_keeping(id, &op_id, "mac", kept.as_ref(), self.now())
            .map_err(failed)?
            .ok_or_else(|| self.no_longer_waiting(id))?;
        scope::note_consent(&used.id, ConsentNote::Used);
        Ok(used)
    }

    fn no_longer_waiting(&self, id: &str) -> ApiError {
        let state = match self.store.get(id) {
            Ok(Some(row)) if row.state.open() && row.expires_ms <= self.now() => "expired",
            Ok(Some(row)) => row.state.as_str(),
            _ => "gone",
        };
        let message = if state == "expired" {
            "That request ran out of time."
        } else {
            "That request is no longer waiting for your yes."
        };
        let mut error = forbidden(message, "needs_consent");
        if let Some(Value::Object(data)) = error.data.as_mut() {
            data.insert("state".into(), json!(state));
        }
        error
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
        self.finish_use_failing(row, outcome.map_err(|_| Failure::unspoken()));
    }

    pub fn finish_use_failing(&self, row: &Consent, outcome: Result<Value, Failure>) {
        match outcome {
            Ok(result) => self.end(row, State::Applied, Some(&result), ConsentThen::Applied),
            Err(failure) => self.end_failed(row, &failure),
        }
    }

    pub fn deny_by_app(&self, id: &str) -> bool {
        self.decide(id, false, None, "mac", None)
            .is_ok_and(|v| v["answered"] == json!(true))
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

    fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::app_only(caller)?;
        let usage =
            "consent.answer takes {consentId, decision: \"allow\" | \"deny\", parts?, note?}";
        let id = args
            .get("consentId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let allow = match args.get("decision").and_then(Value::as_str) {
            Some("allow") => true,
            Some("deny") => false,
            _ => return Err(ApiError::bad_request(usage)),
        };
        let note = match args.get("note") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(
                notchling_engine::consent::clean_note(text)
                    .ok_or_else(|| ApiError::bad_request("The note is empty."))?,
            ),
            Some(_) => return Err(ApiError::bad_request(usage)),
        };
        self.decide_noted(id, allow, (args.get("parts"), note.as_deref()), "mac", None)
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
            lock(&self.restored).insert(row.id.clone());
            self.raise(row);
        }
        self.wake();
    }

    pub fn tick(&self) {
        self.sweep_live();
        if let Ok(due) = self.store.expire_due(self.now()) {
            for row in &due {
                self.closed(row, ConsentThen::Expired);
            }
        }
        self.settle_held();
        let handlers: Vec<Arc<dyn Handler>> = lock(&self.handlers).values().cloned().collect();
        let now = self.now();
        for handler in handlers {
            handler.tick(now);
        }
    }

    fn settle_held(&self) {
        let held = self
            .store
            .open_rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                r.state == State::Applying
                    && r.applied_by == notchling_engine::consent::AppliedBy::App
                    && r.used_op_id.is_none()
            });
        for row in held {
            match self.handler(&row.kind).and_then(|h| h.landed(&row)) {
                Some(result) => self.end(&row, State::Applied, Some(&result), ConsentThen::Applied),
                None if self.now() >= row.answered_ms.unwrap_or(row.created_ms) + HELD_MS => {
                    self.end(&row, State::Withdrawn, None, ConsentThen::Withdrawn)
                }
                None => {}
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
                    Ok(Msg::Withdrawn(id)) => ops.withdrawn_at_allow(&id),
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => ops.tick(),
                }
            })
            .ok();
    }

    pub fn close(&self) {
        self.applies.stop_new();
        let queue = lock(&self.queue).take();
        if let Some(queue) = &queue {
            let _ = queue.send(Msg::Stop);
        }
        drop(queue);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
        self.applies.close();
    }

    pub fn app_attached(&self) -> bool {
        self.room.can_answer()
    }
}

pub fn store_for(data_dir: &Path) -> ConsentStore {
    ConsentStore::open(data_dir).unwrap_or_else(|_| ConsentStore::unavailable())
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
#[path = "consent_outcome_tests.rs"]
mod outcome_tests;

#[cfg(test)]
#[path = "consent_ops_review_tests.rs"]
mod review_tests;

#[cfg(test)]
#[path = "consent_scoped_tests.rs"]
mod scoped_tests;

#[cfg(test)]
#[path = "consent_presence_tests.rs"]
mod presence_tests;

#[cfg(test)]
#[path = "consent_note_tests.rs"]
mod note_tests;
