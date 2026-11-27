use crate::acting_events::{log_down, session_ref, who};
use crate::acting_ops::{bad, text_arg, ActingOps};
use crate::outbox_ops::{via_of, OutboxOps, SendArgs};
use crate::outbox_settle::Settled;
use notchling_engine::outbox::{holds_command, Row, State, Why, REPLY_WAIT_MS};
use notchling_engine::reply::exec::Failure;
use notchling_engine::transcript::stored::is_blank;
use notchling_events::kinds::choices::FailWhy;
use notchling_events::{Ref, Session};
use notchling_local_api::{ApiError, Caller, ErrorCode, Tier};
use serde_json::{json, Value};

const SAVE_FAILED: &str = "Couldn't save the message.";
const MAY_BE_IN: &str =
    "It may already be in the session. Check the session before sending it again.";

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn clear_box_arg(args: &Value) -> Result<bool, ApiError> {
    match args.get("clear_box") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(on)) => Ok(*on),
        Some(_) => Err(bad("clear_box is true or false")),
    }
}

fn queued(outbox: &OutboxOps, row: &Row) -> Value {
    json!({
        "sent": true,
        "state": "queued",
        "waiting": outbox.position(row),
        "outboxId": row.id,
    })
}

pub(crate) fn retrying(row: &Row) -> bool {
    let pending = matches!(row.state, State::Waiting | State::Sending | State::Queued);
    let waits = row.state == State::Waiting && row.why == Some(Why::Retry);
    row.tries > 0 && (waits || (pending && row.unsent))
}

fn failing(row: &Row) -> Value {
    json!({
        "sent": false,
        "state": "retrying",
        "tries": row.tries,
        "outboxId": row.id,
    })
}

fn refusal(row: &Row, note: Option<String>) -> ApiError {
    match row.state {
        State::Box => {
            let own = note
                .as_deref()
                .is_some_and(|text| holds_command(row.tool, text, &row.text));
            let (failure, why) = match own {
                true => (Failure::NotSent, "not_sent"),
                false => (Failure::NotCleared, "not_cleared"),
            };
            let mut data = json!({"why": why});
            if let Some(text) = note {
                data["box"] = json!(text);
            }
            failed(failure.to_string()).with_data(data)
        }
        State::Stopped => failed("The session is stopped."),
        State::Gone => failed("No such session."),
        State::Unsure => failed(MAY_BE_IN),
        _ => failed(Failure::NotSent.to_string()),
    }
}

impl ActingOps {
    fn reply_session(&self, args: &Value) -> Ref<Session> {
        if let Ok(agent) = self.agent(args) {
            return session_ref(&agent);
        }
        match args.get("id").and_then(Value::as_str) {
            Some(id) if id.starts_with("codex:") || id.starts_with("agy:") => Ref::new(id),
            _ => Ref::new("unknown"),
        }
    }

    pub(crate) fn reply(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let text = text_arg(args, "text")?;
        if is_blank(text) {
            return Err(bad("the reply is empty"));
        }
        let clear_box = clear_box_arg(args)?;
        let id = text_arg(args, "id")?;
        let via = who(caller, args).via;
        let session = self.reply_session(args);
        let chars = text.chars().count();
        if caller.tier == Tier::Agent
            && !self.note_reply(session.clone(), via, chars, None, None, true)
        {
            return Err(log_down());
        }
        let Some(outbox) = self.outbox() else {
            return Err(failed(SAVE_FAILED));
        };
        let send = SendArgs {
            id: id.to_string(),
            text: text.to_string(),
            clear_box,
            options: None,
            via: via_of(caller, args),
        };
        let row = match outbox.send_awaited(&send) {
            Ok(row) => row,
            Err(e) => {
                self.note_reply(session, via, chars, Some(FailWhy::Other), None, false);
                return Err(e);
            }
        };
        match outbox.wait_settled(&row, REPLY_WAIT_MS) {
            Settled::Sent => Ok(json!({"sent": true})),
            Settled::Removed => Err(failed("The message was removed.")),
            Settled::Pending(row) if retrying(&row) => Ok(failing(&row)),
            Settled::InQueue(row) | Settled::Pending(row) => Ok(queued(outbox, &row)),
            Settled::NeedsUser(row, note) => match outbox.give_back(&row) {
                true => Err(refusal(&row, note)),
                false => Ok(queued(outbox, &row)),
            },
        }
    }

    pub(crate) fn held_discard(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| bad("session.held.discard takes {\"id\"}"))?;
        let Some(outbox) = self.outbox() else {
            return Ok(json!({"discarded": false}));
        };
        let count = outbox.discard_session(id)?;
        Ok(json!({"discarded": count > 0}))
    }
}

#[cfg(test)]
#[path = "reply_outbox_command_tests.rs"]
mod command_tests;

#[cfg(test)]
#[path = "reply_outbox_tests.rs"]
mod tests;
