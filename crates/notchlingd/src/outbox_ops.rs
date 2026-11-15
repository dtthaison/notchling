use crate::acting::{Acting, ACTING_OFF};
use crate::acting_events::{session_ref, who};
use crate::acting_ops::{bad, forbidden, now_ms, Room};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::outbox_rows::{keeps_note, position_of, positions, reply_via, wire};
use notchling_engine::contract::Tool as RowTool;
use notchling_engine::control::slash::opens_menu;
use notchling_engine::outbox::{
    Change, InsertError, New, OutboxStore, Row, State, Tool, Via, Why, MAX_TEXT,
};
use notchling_engine::sessions::{Agent, Roots};
use notchling_events::field::Count;
use notchling_events::kinds::agents::{OutboxThen, SessionOutbox};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event, Ref, Session};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub type Wake = Arc<dyn Fn(&str) + Send + Sync>;
pub type Resume = Arc<dyn Fn(&Row) -> Result<(), ApiError> + Send + Sync>;

const SAVE_FAILED: &str = "Couldn't save the message.";
const EMPTY: &str = "Type a message first.";
const TOO_LONG: &str = "That message is over 100 KB.";
const NO_MESSAGE: &str = "No such message.";
const BEING_SENT: &str = "It's being sent.";
const IN_QUEUE: &str = "It's in the session's queue.";

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
    wake: Mutex<Option<Wake>>,
    resume: Mutex<Option<Resume>>,
    saving: bool,
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
            wake: Mutex::new(None),
            resume: Mutex::new(None),
            saving: true,
        }
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

    pub fn set_wake(&self, wake: Wake) {
        *lock(&self.wake) = Some(wake);
    }

    pub fn set_resume(&self, resume: Resume) {
        *lock(&self.resume) = Some(resume);
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
    }

    pub fn publish_removed(&self, id: &str, session: &str, why: &str) {
        self.set_note(id, None);
        self.room.publish(
            topics::OUTBOX,
            topics::OUTBOX_REMOVED,
            &json!({"id": id, "session": session, "why": why}),
        );
    }

    pub fn emit_then(&self, row: &Row, then: OutboxThen) {
        self.emit_chars(row, then, row.text.chars().count());
    }

    fn emit_chars(&self, row: &Row, then: OutboxThen, chars: usize) {
        let Some(events) = &self.events else {
            return;
        };
        events.push(Event::new(
            &SessionOutbox {
                session: self.session_ref_of(&row.session),
                then,
                chars: Count(chars as u64),
                via: reply_via(row.via),
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

    fn key_of(&self, id: &str) -> String {
        self.agent_by_id(id)
            .map_or_else(|| id.to_string(), |a| a.outbox_key())
    }

    fn target(&self, id: &str, text: &str) -> Result<Target, ApiError> {
        if is_draft(id) {
            return Err(bad("Start the session with codex.send or agy.send."));
        }
        let tool = tool_of(id);
        let key = match self.agent_by_id(id) {
            Some(agent) => {
                if tool == Tool::Claude && !agent.can_reply() {
                    return Err(bad("Reply in its terminal."));
                }
                agent.outbox_key()
            }
            None if tool != Tool::Claude => id.to_string(),
            None => return Err(bad("No such session.")),
        };
        if tool == Tool::Claude && opens_menu(text) {
            return Err(bad(
                "That command opens a menu; open the session's terminal.",
            ));
        }
        Ok(Target { key, tool })
    }

    pub fn send(&self, args: &SendArgs) -> Result<Row, ApiError> {
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
        let row = self.store.insert(&new, now_ms()).map_err(|e| match e {
            InsertError::Empty => bad(EMPTY),
            InsertError::TooLong => bad(TOO_LONG),
            InsertError::Full => failed("200 messages are already waiting for this session."),
            InsertError::Db(_) => save_failed(),
        })?;
        self.publish_changed(&row);
        self.emit_then(&row, OutboxThen::Queued);
        self.wake_session(&row.session);
        Ok(row)
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
        let row = self.send(&send)?;
        let position = self
            .store
            .list(Some(&row.session))
            .map_or(1, |rows| position_of(&rows, &row.id));
        Ok(json!({
            "outboxId": row.id,
            "session": row.session,
            "state": "queued",
            "position": position,
        }))
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
        let rows: Vec<Value> = rows
            .iter()
            .zip(positions(&rows))
            .map(|(row, position)| wire(row, position, notes.get(&row.id).map(String::as_str)))
            .collect();
        Ok(Value::Array(rows))
    }

    fn row(&self, args: &Value) -> Result<Row, ApiError> {
        let id = text_arg(args, "id")?;
        self.store
            .get(id)
            .map_err(|_| save_failed())?
            .ok_or_else(|| bad(NO_MESSAGE))
    }

    fn busy(row: &Row) -> ApiError {
        failed(if row.state == State::InQueue {
            IN_QUEUE
        } else {
            BEING_SENT
        })
    }

    fn edit(&self, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        check_text(text)?;
        let row = self.row(args)?;
        match self
            .store
            .edit(&row.id, text, now_ms())
            .map_err(|_| save_failed())?
        {
            Change::Done => {}
            Change::Busy => return Err(OutboxOps::busy(&row)),
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
            (Change::Busy, _) => return Err(OutboxOps::busy(&row)),
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
        let key = self.key_of(text_arg(args, "session")?);
        let rows = self
            .store
            .discard_session(&key)
            .map_err(|_| save_failed())?;
        for row in &rows {
            self.publish_removed(&row.id, &row.session, "discarded");
            self.emit_then(row, OutboxThen::Discarded);
        }
        Ok(json!({"discarded": rows.len()}))
    }
}

fn via_of(caller: &Caller, args: &Value) -> Via {
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
