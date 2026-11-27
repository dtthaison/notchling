use crate::acting::{Acting, ACTING_OFF};
use crate::acting_events::session_ref;
use crate::acting_ops::{bad, forbidden, now_ms, Room};
use crate::ask_events::{emit, Facts};
use crate::ask_find::Finder;
use crate::ask_inbox::Link;
use crate::ask_rows::{key_for, wire};
use crate::ask_tick::{queue, Msg, NEVER};
use crate::ask_waiters::{Waiters, ANY};
use crate::caller_session::{CallerSession, SessionHit};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::outbox_ops::Clock;
use crate::presence::{Presence, GRACE_MS};
use crate::vault_caller::Processes;
use notchling_engine::ask::{
    answer_text, branch, check_new, may_answer, AnswerError, AskState, AskStore, By, Mode, New,
    OpenError, Refused, Row, Who,
};
use notchling_engine::sessions::{Agent, Roots};
use notchling_events::kinds::choices::AskThen;
use notchling_events::{Ref, Session};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::AtomicI64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;

pub(crate) const NO_SESSION: &str = "Notchling can't tell which session is asking.";
pub(crate) const PERSON_ONLY: &str = "Only the person answers this question.";
const NO_QUESTION: &str = "No such question.";
const NEEDS_QUESTION: &str = "Ask a question first.";
const QUESTION_TOO_LONG: &str = "That question is over 2,000 characters.";
const GIVE_CHOICES: &str = "Give 2 to 6 choices.";
const CHOICE_TEXT: &str = "A choice is empty or over 120 characters.";
const WORKER_CHOICES: &str =
    "On a Worker, give choices (--choice): the answer comes from your Mac's Inbox.";
const SESSION_FULL: &str = "20 questions from this session are already waiting.";
const ALL_FULL: &str = "100 questions are already waiting.";
const SAVE_QUESTION_FAILED: &str = "Couldn't save the question.";
const SAVE_ANSWER_FAILED: &str = "Couldn't save the answer.";
const NEEDS_ANSWER: &str = "Answer with a choice or text.";
const ANSWER_TOO_LONG: &str = "That answer is over 10 KB.";
const NO_CHOICE: &str = "No such choice.";
const KEY_NEEDED: &str = "key is needed";
const ID_NEEDED: &str = "id is needed";
const KEY_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    Won,
    Lost { state: AskState, by: Option<By> },
}

#[derive(Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) state: AskState,
    pub(crate) by: Option<By>,
}

pub type Worker = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub struct AskOps {
    pub(crate) store: Arc<AskStore>,
    sessions: Arc<Mutex<Value>>,
    pub(crate) acting: Arc<Acting>,
    pub(crate) room: Arc<dyn Room>,
    events: Option<Events>,
    pub(crate) caller: CallerSession,
    pub(crate) procs: Arc<dyn Processes>,
    clock: Clock,
    worker: Worker,
    marks: Mutex<HashMap<String, Mark>>,
    pub(crate) link: OnceLock<Link>,
    pub(crate) finder: OnceLock<Finder>,
    pub(crate) raised: Mutex<HashSet<String>>,
    pub(crate) pending: Mutex<HashSet<String>>,
    saving: bool,
    pub(crate) waiters: Waiters,
    pub(crate) presence: Arc<Presence>,
    pub(crate) tx: Sender<Msg>,
    pub(crate) rx: Mutex<Option<Receiver<Msg>>>,
    pub(crate) thread: Mutex<Option<JoinHandle<()>>>,
    pub(crate) last_sweep: AtomicI64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn failed(message: &str) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn open_error(error: OpenError) -> ApiError {
    match error {
        OpenError::Empty => bad(NEEDS_QUESTION),
        OpenError::TooLong => bad(QUESTION_TOO_LONG),
        OpenError::Choices => bad(GIVE_CHOICES),
        OpenError::ChoiceText => bad(CHOICE_TEXT),
        OpenError::WorkerNeedsChoices => bad(WORKER_CHOICES),
        OpenError::SessionFull => failed(SESSION_FULL).with_reason("busy"),
        OpenError::AllFull => failed(ALL_FULL).with_reason("busy"),
        OpenError::Db(_) => failed(SAVE_QUESTION_FAILED),
    }
}

fn choices_arg(args: &Value) -> Result<Option<Vec<String>>, OpenError> {
    match args.get("choices") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(all)) => all
            .iter()
            .map(|c| c.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .map(Some)
            .ok_or(OpenError::Choices),
        Some(_) => Err(OpenError::Choices),
    }
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn id_arg(args: &Value) -> Result<&str, ApiError> {
    args.get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad(ID_NEEDED))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl AskOps {
    pub fn new(
        store: Arc<AskStore>,
        sessions: Arc<Mutex<Value>>,
        roots: Roots,
        acting: Arc<Acting>,
        room: Arc<dyn Room>,
        events: Option<Events>,
        procs: Arc<dyn Processes>,
    ) -> AskOps {
        let (tx, rx) = queue();
        let clock: Clock = Arc::new(now_ms);
        AskOps {
            presence: Presence::new(GRACE_MS, clock.clone()),
            store,
            sessions,
            acting,
            room,
            events,
            caller: CallerSession::new(roots, procs.clone()),
            procs,
            clock,
            worker: Arc::new(|| None),
            marks: Mutex::new(HashMap::new()),
            link: OnceLock::new(),
            finder: OnceLock::new(),
            raised: Mutex::new(HashSet::new()),
            pending: Mutex::new(HashSet::new()),
            saving: true,
            waiters: Waiters::default(),
            tx,
            rx,
            thread: Mutex::new(None),
            last_sweep: AtomicI64::new(NEVER),
        }
    }

    pub fn with_clock(mut self, clock: Clock) -> AskOps {
        self.presence = Presence::new(GRACE_MS, clock.clone());
        self.clock = clock;
        self
    }

    pub fn with_worker(mut self, worker: Worker) -> AskOps {
        self.worker = worker;
        self
    }

    pub fn without_store(mut self) -> AskOps {
        self.saving = false;
        self
    }

    pub fn store(&self) -> &Arc<AskStore> {
        &self.store
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    fn on_worker(&self) -> bool {
        (self.worker)().is_some()
    }

    pub(crate) fn node_name(&self) -> Option<String> {
        (self.worker)()
    }

    pub(crate) fn agents(&self) -> Vec<Agent> {
        let list = lock(&self.sessions);
        list.as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn agent_by_id(&self, id: &str) -> Option<Agent> {
        let list = lock(&self.sessions);
        let row = list
            .as_array()?
            .iter()
            .find(|r| r.get("id") == Some(&json!(id)))?;
        serde_json::from_value(row.clone()).ok()
    }

    pub(crate) fn session_key(&self, hit: &SessionHit) -> String {
        key_for(&self.agents(), &hit.session_id)
    }

    fn key_of(&self, id: &str) -> String {
        self.agent_by_id(id)
            .map_or_else(|| id.to_string(), |a| a.outbox_key())
    }

    pub fn session_ref_of(&self, key: &str) -> Ref<Session> {
        self.agents()
            .into_iter()
            .find(|a| a.outbox_key() == key)
            .map_or_else(|| Ref::new(key), |a| session_ref(&a))
    }

    pub fn sessions_changed(&self) {
        let Ok(rows) = self.store.live() else {
            return;
        };
        let agents = self.agents();
        let mut moved = HashSet::new();
        for row in &rows {
            if row.session_id.as_deref() != Some(row.session.as_str()) {
                continue;
            }
            let key = key_for(&agents, &row.session);
            if key == row.session || !moved.insert(row.session.clone()) {
                continue;
            }
            if !matches!(self.store.rekey(&row.session, &key), Ok(n) if n > 0) {
                continue;
            }
            for old in rows.iter().filter(|r| r.session == row.session) {
                if let Ok(Some(changed)) = self.store.get(&old.id) {
                    self.publish_changed(&changed);
                }
            }
        }
    }

    fn waiting(&self, id: &str) -> bool {
        self.waiters.connected(id)
    }

    pub fn publish_changed(&self, row: &Row) {
        self.waiters.wake(ANY);
        self.room.publish(
            topics::ASKS,
            topics::ASK_CHANGED,
            &json!({"row": wire(row, self.waiting(&row.id), true)}),
        );
        self.tell_owner(row);
    }

    fn tell_owner(&self, row: &Row) {
        let Some(owner) = row.session_id.as_deref() else {
            return;
        };
        let data =
            json!({"row": wire(row, self.waiting(&row.id), false), "state": row.state.as_str()});
        self.room
            .publish_to(owner, topics::MINE_ASKS, topics::ASK_CHANGED, &data);
    }

    pub fn emit_then(&self, row: &Row, then: AskThen) {
        self.emit_at(row, then, self.now());
    }

    pub(crate) fn emit_at(&self, row: &Row, then: AskThen, now: i64) {
        let facts = Facts::of(row, self.session_ref_of(&row.session), then, now);
        emit(self.events.as_ref(), facts, now);
    }

    pub(crate) fn moved(&self, row: &Row, then: AskThen) {
        self.publish_changed(row);
        self.emit_then(row, then);
        self.waiters.wake(&row.id);
        self.resolve_item(row);
    }

    fn refuse(&self, hit: Option<&SessionHit>, mode: Mode, args: &Value) {
        let session = match hit {
            Some(hit) => self.session_ref_of(&self.session_key(hit)),
            None => Ref::new("unknown"),
        };
        let chars = args
            .get("question")
            .and_then(Value::as_str)
            .map_or(0, |q| q.chars().count());
        let choices = args
            .get("choices")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let facts = Facts::refused(session, mode, chars, choices);
        emit(self.events.as_ref(), facts, self.now());
    }

    fn opened(&self, row: &Row, hit: &SessionHit) {
        self.publish_changed(row);
        self.emit_then(row, AskThen::Asked);
        if !self.start_find(row, hit) {
            self.settled(&row.id);
        }
    }

    fn open(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let mode = if flag(args, "wait") {
            Mode::Wait
        } else {
            Mode::Background
        };
        let pid = caller.pid.filter(|pid| *pid > 0);
        let hit = pid.and_then(|pid| self.caller.session_of(pid));
        let (Some(pid), Some(hit)) = (pid, hit) else {
            self.refuse(None, mode, args);
            return Err(forbidden(NO_SESSION).with_reason("no_session"));
        };
        let refuse = |error: ApiError| {
            self.refuse(Some(&hit), mode, args);
            error
        };
        let question = args.get("question").and_then(Value::as_str).unwrap_or("");
        let choices = choices_arg(args).map_err(|e| refuse(open_error(e)))?;
        check_new(question, choices.as_deref(), self.on_worker())
            .map_err(|e| refuse(open_error(e)))?;
        let key = args
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty() && k.len() <= KEY_MAX)
            .ok_or_else(|| bad(KEY_NEEDED))?;
        if !self.saving {
            return Err(refuse(failed(SAVE_QUESTION_FAILED)));
        }
        let known = self
            .store
            .by_key(key, pid)
            .map_err(|_| failed(SAVE_QUESTION_FAILED))?;
        if let Some(row) = known {
            return Ok(json!({"id": row.id, "state": row.state.as_str()}));
        }
        let session = self.session_key(&hit);
        let folder = self.procs.cwd(pid);
        let new = New {
            key: key.to_string(),
            session,
            session_id: Some(hit.session_id.clone()),
            pid,
            pid_start: self.procs.start_time(pid),
            branch: folder.as_deref().and_then(branch),
            folder: folder.as_deref().map(path_text),
            question: question.to_string(),
            choices,
            main_may_answer: flag(args, "mainMayAnswer"),
            mode,
            deadline_ms: args.get("deadlineMs").and_then(Value::as_i64),
        };
        let row = self
            .store
            .insert(&new, self.now())
            .map_err(|e| refuse(open_error(e)))?;
        self.opened(&row, &hit);
        Ok(json!({"id": row.id, "state": "open"}))
    }

    pub(crate) fn take(&self, id: &str, mark: Mark) -> Option<Mark> {
        let mut marks = lock(&self.marks);
        if let Some(taken) = marks.get(id) {
            return Some(*taken);
        }
        marks.insert(id.to_string(), mark);
        None
    }

    pub(crate) fn drop_mark(&self, id: &str) {
        lock(&self.marks).remove(id);
    }

    fn lost(&self, id: &str) -> Result<Taken, ApiError> {
        let row = self.row(id)?;
        Ok(Taken::Lost {
            state: row.state,
            by: row.by,
        })
    }

    pub(crate) fn row(&self, id: &str) -> Result<Row, ApiError> {
        self.store
            .get(id)
            .map_err(|_| failed(SAVE_ANSWER_FAILED))?
            .ok_or_else(|| bad(NO_QUESTION))
    }

    pub fn answer(
        &self,
        id: &str,
        text: Option<&str>,
        choice: Option<u8>,
        by: By,
    ) -> Result<Taken, ApiError> {
        self.answer_as(id, text, choice, by, &Who::Person)
    }

    pub(crate) fn answer_as(
        &self,
        id: &str,
        text: Option<&str>,
        choice: Option<u8>,
        by: By,
        who: &Who,
    ) -> Result<Taken, ApiError> {
        let row = self.row(id)?;
        match may_answer(&row, who) {
            Ok(()) => {}
            Err(Refused::NotOpen(state)) => return Ok(Taken::Lost { state, by: row.by }),
            Err(Refused::PersonOnly) => return Err(forbidden(PERSON_ONLY)),
        }
        let (answer, choice) = answer_text(&row, text, choice).map_err(|e| match e {
            AnswerError::Missing => bad(NEEDS_ANSWER),
            AnswerError::TooLong => bad(ANSWER_TOO_LONG),
            AnswerError::NoSuchChoice => bad(NO_CHOICE),
        })?;
        let mark = Mark {
            state: AskState::Answered,
            by: Some(by),
        };
        if let Some(taken) = self.take(id, mark) {
            return Ok(Taken::Lost {
                state: taken.state,
                by: taken.by,
            });
        }
        self.finish_answer(id, &answer, choice, by)
    }

    pub(crate) fn finish_answer(
        &self,
        id: &str,
        answer: &str,
        choice: Option<u8>,
        by: By,
    ) -> Result<Taken, ApiError> {
        let wrote = self.store.answer(id, answer, choice, by, self.now());
        if !matches!(wrote, Ok(true)) {
            self.drop_mark(id);
        }
        match wrote {
            Err(_) => Err(failed(SAVE_ANSWER_FAILED)),
            Ok(false) => self.lost(id),
            Ok(true) => {
                if let Ok(Some(changed)) = self.store.get(id) {
                    self.moved(&changed, AskThen::Answered);
                }
                Ok(Taken::Won)
            }
        }
    }

    pub fn dismiss(&self, id: &str) -> Result<bool, ApiError> {
        Ok(self.dismiss_taken(id)? == Taken::Won)
    }

    pub fn dismiss_taken(&self, id: &str) -> Result<Taken, ApiError> {
        let row = self.row(id)?;
        if let Err(refused) = may_answer(&row, &Who::Person) {
            let state = match refused {
                Refused::NotOpen(state) => state,
                Refused::PersonOnly => row.state,
            };
            return Ok(Taken::Lost { state, by: row.by });
        }
        let mark = Mark {
            state: AskState::Dismissed,
            by: None,
        };
        if let Some(taken) = self.take(id, mark) {
            return Ok(Taken::Lost {
                state: taken.state,
                by: taken.by,
            });
        }
        self.finish_dismiss(id)
    }

    pub(crate) fn finish_dismiss(&self, id: &str) -> Result<Taken, ApiError> {
        let closed = self
            .store
            .close(id, &[AskState::Open], AskState::Dismissed, self.now());
        if !matches!(closed, Ok(true)) {
            self.drop_mark(id);
        }
        match closed {
            Err(_) => Err(failed(SAVE_ANSWER_FAILED)),
            Ok(false) => self.lost(id),
            Ok(true) => {
                if let Ok(Some(changed)) = self.store.get(id) {
                    self.moved(&changed, AskThen::Dismissed);
                }
                Ok(Taken::Won)
            }
        }
    }

    fn dismiss_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier == Tier::Agent {
            return Err(forbidden(PERSON_ONLY));
        }
        let id = id_arg(args)?;
        Ok(match self.dismiss_taken(id)? {
            Taken::Won => json!({"dismissed": true}),
            Taken::Lost { state, by } => {
                let mut out = json!({"dismissed": false, "state": state.as_str()});
                if let Some(by) = by {
                    out["by"] = json!(by.as_str());
                }
                out
            }
        })
    }

    fn list_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier == Tier::Agent {
            return Err(forbidden(PERSON_ONLY));
        }
        let session = args
            .get("session")
            .and_then(Value::as_str)
            .map(|s| self.key_of(s));
        let rows = self
            .store
            .list(session.as_deref())
            .map_err(|_| failed(SAVE_ANSWER_FAILED))?;
        Ok(Value::Array(
            rows.iter()
                .map(|row| wire(row, self.waiting(&row.id), true))
                .collect(),
        ))
    }
}

impl OpGroup for AskOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::ASK_WAIT | ops::ASK_MINE => Some(OpSpec::AGENT_READ),
            ops::ASK_OPEN | ops::ASK_ANSWER => Some(OpSpec::AGENT_ACTS),
            ops::ASK_DISMISS | ops::ASK_LIST => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op == ops::ASK_LIST {
            return self.list_op(caller, args);
        }
        if op == ops::ASK_WAIT {
            return self.wait_op(caller, args);
        }
        if op == ops::ASK_MINE {
            return self.mine_op(caller, args);
        }
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        match op {
            ops::ASK_OPEN => self.open(caller, args),
            ops::ASK_ANSWER => self.answer_op(caller, args),
            ops::ASK_DISMISS => self.dismiss_op(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::ASKS]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::ASKS).then_some(Tier::App)
    }
}

#[cfg(test)]
#[path = "ask_ops_tests.rs"]
mod tests;
