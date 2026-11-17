use crate::acting::{Acting, ACTING_OFF};
use crate::acting_events::{session_ref, who};
use crate::acting_ops::{bad, forbidden, now_ms, Room};
use crate::agy_ops::valid_session;
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::outbox_rows::{keeps_note, position_of, positions, reply_via, wire};
use crate::outbox_settle::Settle;
use crate::outbox_verify::count_row;
use notchling_engine::cluster::Qualified;
use notchling_engine::contract::Tool as RowTool;
use notchling_engine::control::slash::opens_menu;
use notchling_engine::outbox::{
    verdict, Change, InsertError, New, OutboxStore, Row, State, Tool, Verdict, Via, Why,
    BEING_SENT, IN_QUEUE, MAX_TEXT, NO_MESSAGE, VERIFY_LOOK_MS,
};
use notchling_engine::sessions::{fresh, Agent, Probes, Roots};
use notchling_engine::tools::codex::plan::valid_session_id;
use notchling_engine::transcript::sent::Seen;
use notchling_events::field::Count;
use notchling_events::kinds::agents::{OutboxThen, SessionOutbox};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event, Ref, Session};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub type Wake = Arc<dyn Fn(&str) + Send + Sync>;
pub type Check = Arc<dyn Fn(&str) -> Result<(), ApiError> + Send + Sync>;
pub type Resume = Arc<dyn Fn(&Row) -> Result<(), ApiError> + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type Tell = Arc<dyn Fn(&str, &str, &Value) + Send + Sync>;

pub trait RemoteRows: Send + Sync {
    fn rows(&self, session: Option<&str>) -> Vec<Value>;
}

const SAVE_FAILED: &str = "Couldn't save the message.";
const EMPTY: &str = "Type a message first.";
const TOO_LONG: &str = "That message is over 100 KB.";
const NO_SESSION: &str = "No such session.";
const MENU: &str = "That command opens a menu; open the session's terminal.";

pub struct SendArgs {
    pub id: String,
    pub text: String,
    pub clear_box: bool,
    pub options: Option<Value>,
    pub via: Via,
}

struct Target {
    key: String,
    tool: Tool,
}

pub struct OutboxOps {
    store: Arc<OutboxStore>,
    sessions: Arc<Mutex<Value>>,
    roots: Roots,
    acting: Arc<Acting>,
    room: Arc<dyn Room>,
    events: Option<Events>,
    notes: Mutex<HashMap<String, String>>,
    apart: Mutex<HashSet<String>>,
    wake: Mutex<Option<Wake>>,
    forget: Mutex<Option<Wake>>,
    resume: Mutex<Option<Resume>>,
    tell: Mutex<Option<Tell>>,
    remote: Mutex<Option<Arc<dyn RemoteRows>>>,
    codex_check: OnceLock<Check>,
    agy_check: OnceLock<Check>,
    saving: bool,
    clock: Clock,
    settle: Settle,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn failed(message: &str) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn save_failed() -> ApiError {
    failed(SAVE_FAILED)
}

fn tool_of(id: &str) -> Tool {
    match RowTool::of_row(id) {
        RowTool::Claude => Tool::Claude,
        RowTool::Codex => Tool::Codex,
        RowTool::Antigravity => Tool::Antigravity,
    }
}

fn is_draft(id: &str) -> bool {
    ["codex-new:", "agy-new:", "antigravity-new:"]
        .iter()
        .any(|p| id.starts_with(p))
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{key} is needed")))
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn foreign_key(id: &str) -> Option<String> {
    if let Some(session) = id.strip_prefix("codex:") {
        return valid_session_id(session).then(|| format!("codex:{session}"));
    }
    let session = id
        .strip_prefix("agy:")
        .or_else(|| id.strip_prefix("antigravity:"))?;
    valid_session(session).then(|| format!("agy:{session}"))
}

fn refuse_menu(tool: Tool, text: &str) -> Result<(), ApiError> {
    if tool == Tool::Claude && opens_menu(text) {
        return Err(bad(MENU));
    }
    Ok(())
}

fn check_text(text: &str) -> Result<(), ApiError> {
    if text.trim().is_empty() {
        return Err(bad(EMPTY));
    }
    if text.len() > MAX_TEXT {
        return Err(bad(TOO_LONG));
    }
    Ok(())
}

impl OutboxOps {
    pub fn new(
        store: Arc<OutboxStore>,
        sessions: Arc<Mutex<Value>>,
        roots: Roots,
        acting: Arc<Acting>,
        room: Arc<dyn Room>,
        events: Option<Events>,
    ) -> OutboxOps {
        OutboxOps {
            store,
            sessions,
            roots,
            acting,
            room,
            events,
            notes: Mutex::new(HashMap::new()),
            apart: Mutex::new(HashSet::new()),
            wake: Mutex::new(None),
            forget: Mutex::new(None),
            resume: Mutex::new(None),
            tell: Mutex::new(None),
            remote: Mutex::new(None),
            codex_check: OnceLock::new(),
            agy_check: OnceLock::new(),
            saving: true,
            clock: Arc::new(now_ms),
            settle: Settle::default(),
        }
    }

    pub fn with_clock(mut self, clock: Clock) -> OutboxOps {
        self.clock = clock;
        self
    }

    pub(crate) fn now(&self) -> i64 {
        (self.clock)()
    }

    pub(crate) fn settle(&self) -> &Settle {
        &self.settle
    }

    pub fn without_store(mut self) -> OutboxOps {
        self.saving = false;
        self
    }

    pub fn store(&self) -> &Arc<OutboxStore> {
        &self.store
    }

    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    pub fn acting(&self) -> &Arc<Acting> {
        &self.acting
    }

    pub fn load(&self, now_ms: i64) {
        let Ok(rows) = self.store.sending() else {
            return;
        };
        for row in rows {
            self.settle_sending(&row, now_ms);
        }
    }

    pub(crate) fn settle_sending(&self, row: &Row, now_ms: i64) -> bool {
        let seen = match row.tool {
            Tool::Claude => fresh::job_row(&self.roots, &Probes::from_proc(), &row.session)
                .and_then(|agent| count_row(agent.transcript_path.as_deref()?, &agent.name, row)),
            Tool::Codex | Tool::Antigravity => None,
        };
        let before = match (row.seen_before, row.queued_before) {
            (Some(taken), Some(enqueued)) => Some(Seen {
                taken: usize::try_from(taken).unwrap_or(0),
                enqueued: usize::try_from(enqueued).unwrap_or(0),
            }),
            _ => None,
        };
        let judged = before.map_or(Verdict::Unsure, |b| verdict(seen, b, VERIFY_LOOK_MS));
        let (to, why, then) = match judged {
            Verdict::Sent => {
                let Ok(true) = self.store.delete(&row.id) else {
                    return false;
                };
                self.publish_removed(&row.id, &row.session, "sent");
                self.emit_then(row, OutboxThen::Sent);
                return true;
            }
            Verdict::InQueue => (State::InQueue, None, OutboxThen::InQueue),
            Verdict::Pending | Verdict::Unsure => {
                (State::Unsure, Some(Why::Restart), OutboxThen::Unsure)
            }
        };
        let moved = self
            .store
            .set_state(&row.id, &[State::Sending], to, why, None, now_ms);
        let Ok(true) = moved else {
            return false;
        };
        if let Ok(Some(changed)) = self.store.get(&row.id) {
            self.publish_changed(&changed);
        }
        self.emit_then(row, then);
        true
    }

    pub fn set_wake(&self, wake: Wake) {
        *lock(&self.wake) = Some(wake);
    }

    pub fn set_forget(&self, forget: Wake) {
        *lock(&self.forget) = Some(forget);
    }

    pub fn set_resume(&self, resume: Resume) {
        *lock(&self.resume) = Some(resume);
    }

    pub fn set_tell(&self, tell: Tell) {
        *lock(&self.tell) = Some(tell);
    }

    pub fn set_remote(&self, remote: Arc<dyn RemoteRows>) {
        *lock(&self.remote) = Some(remote);
    }

    fn tell_peers(&self, name: &str, data: &Value) {
        let tell = lock(&self.tell).clone();
        if let Some(tell) = tell {
            tell(topics::OUTBOX, name, data);
        }
    }

    pub fn set_codex_check(&self, check: Check) {
        let _ = self.codex_check.set(check);
    }

    pub fn set_agy_check(&self, check: Check) {
        let _ = self.agy_check.set(check);
    }

    fn check_foreign(&self, tool: Tool, key: &str) -> Result<(), ApiError> {
        let (check, prefix) = match tool {
            Tool::Codex => (self.codex_check.get(), "codex:"),
            _ => (self.agy_check.get(), "agy:"),
        };
        match check {
            Some(check) => check(key.strip_prefix(prefix).unwrap_or(key)),
            None => Ok(()),
        }
    }

    pub fn position(&self, row: &Row) -> usize {
        self.store
            .list(Some(&row.session))
            .map_or(1, |rows| position_of(&rows, &row.id))
    }

    pub fn set_note(&self, id: &str, note: Option<String>) {
        let mut notes = lock(&self.notes);
        match note {
            Some(note) => {
                notes.insert(id.to_string(), note);
            }
            None => {
                notes.remove(id);
            }
        }
    }

    pub fn note(&self, id: &str) -> Option<String> {
        lock(&self.notes).get(id).cloned()
    }

    pub fn publish_changed(&self, row: &Row) {
        if !keeps_note(row) {
            self.set_note(&row.id, None);
        }
        let position = self
            .store
            .list(Some(&row.session))
            .map_or(1, |rows| position_of(&rows, &row.id));
        let wire = wire(row, position, self.note(&row.id).as_deref());
        self.room.publish(
            topics::OUTBOX,
            topics::OUTBOX_CHANGED,
            &json!({"row": wire}),
        );
        self.tell_peers(topics::OUTBOX_CHANGED, &json!({}));
        self.settle.bump();
    }

    pub fn publish_removed(&self, id: &str, session: &str, why: &str) {
        lock(&self.apart).remove(id);
        self.settle.removed(id, why);
        self.set_note(id, None);
        let forget = lock(&self.forget).clone();
        if let Some(forget) = forget {
            forget(id);
        }
        self.room.publish(
            topics::OUTBOX,
            topics::OUTBOX_REMOVED,
            &json!({"id": id, "session": session, "why": why}),
        );
        self.tell_peers(
            topics::OUTBOX_REMOVED,
            &json!({"id": id, "session": session, "why": why}),
        );
        self.settle.bump();
    }

    pub fn emit_then(&self, row: &Row, then: OutboxThen) {
        self.emit_chars(row, then, row.text.chars().count(), row.via);
    }

    fn emit_chars(&self, row: &Row, then: OutboxThen, chars: usize, via: Via) {
        let Some(events) = &self.events else {
            return;
        };
        events.push(Event::new(
            &SessionOutbox {
                session: self.session_ref_of(&row.session),
                then,
                chars: Count(chars as u64),
                via: reply_via(via),
            },
            Outcome::Info,
            now_ms(),
        ));
        scope::mark_recorded();
    }

    fn wake_session(&self, session: &str) {
        let wake = lock(&self.wake).clone();
        if let Some(wake) = wake {
            wake(session);
        }
    }

    fn agents(&self) -> Vec<Agent> {
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

    fn session_ref_of(&self, key: &str) -> Ref<Session> {
        self.agents()
            .into_iter()
            .find(|a| a.outbox_key() == key)
            .map_or_else(|| Ref::new(key), |a| session_ref(&a))
    }

    pub fn session_name(&self, key: &str) -> Option<String> {
        let cached = self
            .agents()
            .into_iter()
            .find(|a| a.outbox_key() == key)
            .map(|a| a.name);
        if cached.is_some() || tool_of(key) != Tool::Claude {
            return cached;
        }
        fresh::job_row(&self.roots, &Probes::from_proc(), key).map(|a| a.name)
    }

    pub(crate) fn resend_row(&self, id: &str) -> Result<Value, ApiError> {
        self.resend(&json!({"id": id, "clear_box": true, "resume": true}))
    }

    fn key_of(&self, id: &str) -> String {
        if tool_of(id) != Tool::Claude {
            return foreign_key(id).unwrap_or_else(|| id.to_string());
        }
        self.agent_by_id(id)
            .map_or_else(|| id.to_string(), |a| a.outbox_key())
    }

    fn target(&self, id: &str, text: &str) -> Result<Target, ApiError> {
        if is_draft(id) {
            return Err(bad("Start the session with codex.send or agy.send."));
        }
        let tool = tool_of(id);
        let key = match tool {
            Tool::Claude => match self.agent_by_id(id) {
                Some(agent) if agent.can_reply() => agent.outbox_key(),
                Some(_) => return Err(bad("Reply in its terminal.")),
                None => return Err(bad(NO_SESSION)),
            },
            Tool::Codex | Tool::Antigravity => {
                let key = foreign_key(id).ok_or_else(|| bad(NO_SESSION))?;
                self.check_foreign(tool, &key)?;
                key
            }
        };
        refuse_menu(tool, text)?;
        Ok(Target { key, tool })
    }

    pub fn send(&self, args: &SendArgs) -> Result<Row, ApiError> {
        self.save(args, false).map(|(row, _)| row)
    }

    pub fn send_combining(&self, args: &SendArgs) -> Result<(Row, bool), ApiError> {
        self.save(args, true)
    }

    fn save(&self, args: &SendArgs, combine: bool) -> Result<(Row, bool), ApiError> {
        check_text(&args.text)?;
        let target = self.target(&args.id, &args.text)?;
        if !self.saving {
            return Err(save_failed());
        }
        let new = New {
            session: target.key,
            tool: target.tool,
            text: args.text.clone(),
            options: args.options.clone(),
            via: args.via,
            clear_box: args.clear_box,
        };
        if combine {
            let joined = self
                .store
                .combine_except(&new, now_ms(), &|id| lock(&self.apart).contains(id))
                .map_err(|_| save_failed())?;
            if let Some(row) = joined {
                self.publish_changed(&row);
                self.emit_chars(
                    &row,
                    OutboxThen::Combined,
                    args.text.chars().count(),
                    args.via,
                );
                self.wake_session(&row.session);
                return Ok((row, true));
            }
        }
        let row = self.store.insert(&new, now_ms()).map_err(|e| match e {
            InsertError::Empty => bad(EMPTY),
            InsertError::TooLong => bad(TOO_LONG),
            InsertError::Full => failed("200 messages are already waiting for this session."),
            InsertError::Db(_) => save_failed(),
        })?;
        if !combine {
            lock(&self.apart).insert(row.id.clone());
        }
        self.publish_changed(&row);
        self.emit_then(&row, OutboxThen::Queued);
        self.wake_session(&row.session);
        Ok((row, false))
    }

    fn send_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let via = args
            .get("via")
            .and_then(Value::as_str)
            .and_then(Via::parse)
            .unwrap_or_else(|| via_of(caller, args));
        let send = SendArgs {
            id: text_arg(args, "id")?.to_string(),
            text: text_arg(args, "text")?.to_string(),
            clear_box: flag(args, "clear_box"),
            options: args.get("options").filter(|o| o.is_object()).cloned(),
            via,
        };
        let (row, combined) = self.send_combining(&send)?;
        let position = self.position(&row);
        let mut sent = json!({
            "outboxId": row.id,
            "session": row.session,
            "state": "queued",
            "position": position,
        });
        if combined {
            sent["combined"] = json!(true);
        }
        Ok(sent)
    }

    fn list(&self, args: &Value) -> Result<Value, ApiError> {
        let session = args
            .get("session")
            .and_then(Value::as_str)
            .map(|s| self.key_of(s));
        let rows = self
            .store
            .list(session.as_deref())
            .map_err(|_| save_failed())?;
        let notes = lock(&self.notes).clone();
        let mut rows: Vec<Value> = rows
            .iter()
            .zip(positions(&rows))
            .map(|(row, position)| wire(row, position, notes.get(&row.id).map(String::as_str)))
            .collect();
        rows.extend(self.remote_rows(args.get("session").and_then(Value::as_str)));
        Ok(Value::Array(rows))
    }

    fn remote_rows(&self, asked: Option<&str>) -> Vec<Value> {
        if asked.is_some_and(|s| Qualified::parse(s).is_none()) {
            return Vec::new();
        }
        let remote = lock(&self.remote).clone();
        remote.map_or_else(Vec::new, |remote| remote.rows(asked))
    }

    fn row(&self, args: &Value) -> Result<Row, ApiError> {
        let id = text_arg(args, "id")?;
        self.store
            .get(id)
            .map_err(|_| save_failed())?
            .ok_or_else(|| bad(NO_MESSAGE))
    }

    pub(crate) fn busy_refusal(&self, id: &str) -> ApiError {
        match self.store.get(id) {
            Ok(Some(row)) if row.state == State::InQueue => failed(IN_QUEUE),
            Ok(Some(_)) => failed(BEING_SENT),
            Ok(None) => bad(NO_MESSAGE),
            Err(_) => save_failed(),
        }
    }

    fn edit(&self, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        check_text(text)?;
        let row = self.row(args)?;
        refuse_menu(row.tool, text)?;
        match self
            .store
            .edit(&row.id, text, now_ms())
            .map_err(|_| save_failed())?
        {
            Change::Done => {}
            Change::Busy => return Err(self.busy_refusal(&row.id)),
            Change::Missing => return Err(bad(NO_MESSAGE)),
        }
        if let Ok(Some(edited)) = self.store.get(&row.id) {
            self.publish_changed(&edited);
            self.emit_then(&edited, OutboxThen::Edited);
        }
        self.wake_session(&row.session);
        Ok(json!({"edited": true}))
    }

    fn remove(&self, args: &Value) -> Result<Value, ApiError> {
        let row = self.row(args)?;
        match self.store.remove(&row.id).map_err(|_| save_failed())? {
            (Change::Done, _) => {}
            (Change::Busy, _) => return Err(self.busy_refusal(&row.id)),
            (Change::Missing, _) => return Err(bad(NO_MESSAGE)),
        }
        self.publish_removed(&row.id, &row.session, "removed");
        self.emit_then(&row, OutboxThen::Removed);
        self.wake_session(&row.session);
        Ok(json!({"removed": true}))
    }

    fn resend(&self, args: &Value) -> Result<Value, ApiError> {
        let row = self.row(args)?;
        let (from, clear_box) = match (row.state, row.why) {
            (State::Unsure, _) => (State::Unsure, false),
            (State::Box, _) => {
                if !flag(args, "clear_box") {
                    return Err(bad("The box has other text; clear it to send."));
                }
                (State::Box, true)
            }
            (State::Stopped, _) => {
                if !flag(args, "resume") {
                    return Err(bad("Resume the session to send."));
                }
                let resume = lock(&self.resume)
                    .clone()
                    .ok_or_else(|| failed("Resume isn't available."))?;
                resume(&row)?;
                (State::Stopped, row.clear_box)
            }
            (State::Waiting, Some(Why::Retry)) => (State::Waiting, row.clear_box),
            _ => return Err(bad("There's nothing to send again.")),
        };
        match self
            .store
            .requeue(&row.id, &[from], clear_box, now_ms())
            .map_err(|_| save_failed())?
        {
            Change::Done => {}
            Change::Busy => return Err(failed("The message changed; try again.")),
            Change::Missing => return Err(bad(NO_MESSAGE)),
        }
        if let Ok(Some(queued)) = self.store.get(&row.id) {
            self.publish_changed(&queued);
        }
        self.wake_session(&row.session);
        Ok(json!({"state": "queued"}))
    }

    fn discard(&self, args: &Value) -> Result<Value, ApiError> {
        let count = self.discard_session(text_arg(args, "session")?)?;
        Ok(json!({"discarded": count}))
    }

    pub(crate) fn discard_session(&self, id: &str) -> Result<usize, ApiError> {
        let key = self.key_of(id);
        let rows = self
            .store
            .discard_session(&key)
            .map_err(|_| save_failed())?;
        for row in &rows {
            self.publish_removed(&row.id, &row.session, "discarded");
            self.emit_then(row, OutboxThen::Discarded);
        }
        Ok(rows.len())
    }

    pub fn remove_session(&self, key: &str) {
        let Ok(rows) = self.store.list(Some(key)) else {
            return;
        };
        for row in rows {
            if let Ok(true) = self.store.delete(&row.id) {
                self.publish_removed(&row.id, &row.session, "session_removed");
                self.emit_then(&row, OutboxThen::SessionRemoved);
            }
        }
    }

    pub(crate) fn give_back(&self, row: &Row) -> bool {
        match self.store.remove(&row.id) {
            Ok((Change::Done, _)) => {
                self.publish_removed(&row.id, &row.session, "removed");
                self.emit_then(row, OutboxThen::Removed);
                true
            }
            _ => false,
        }
    }
}

pub(crate) fn via_of(caller: &Caller, args: &Value) -> Via {
    use notchling_events::kinds::agents::ReplyVia;
    match who(caller, args).via {
        ReplyVia::Notch => Via::Notch,
        ReplyVia::Phone => Via::Phone,
        ReplyVia::Agent => Via::Agent,
        _ => Via::Chat,
    }
}

impl OpGroup for OutboxOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(
            op,
            ops::SESSION_SEND
                | ops::OUTBOX_LIST
                | ops::OUTBOX_EDIT
                | ops::OUTBOX_REMOVE
                | ops::OUTBOX_RESEND
                | ops::OUTBOX_DISCARD
        )
        .then_some(OpSpec::APP)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op == ops::OUTBOX_LIST {
            return self.list(args);
        }
        if !self.acting.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        match op {
            ops::SESSION_SEND => self.send_op(caller, args),
            ops::OUTBOX_EDIT => self.edit(args),
            ops::OUTBOX_REMOVE => self.remove(args),
            ops::OUTBOX_RESEND => self.resend(args),
            ops::OUTBOX_DISCARD => self.discard(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::OUTBOX]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::OUTBOX).then_some(Tier::App)
    }
}

#[cfg(test)]
#[path = "outbox_ops_tests.rs"]
mod tests;
