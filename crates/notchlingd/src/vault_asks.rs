use super::*;
use notchling_engine::inbox::{ActionId, State};
use std::time::Instant;

pub(super) struct Asked {
    pub(super) decision: Decision,
    pub(super) state: State,
    pub(super) by: Answerer,
    pub(super) device: Option<String>,
    pub(super) waited_ms: u64,
}

pub(super) struct Refusal {
    pub(super) err: ApiError,
    pub(super) result: SecretResult,
}

fn refusal(err: ApiError, result: SecretResult) -> Refusal {
    Refusal { err, result }
}

impl VaultOps {
    pub(super) fn approve(
        &self,
        caller: &Caller,
        asking: &[VaultEntry],
        command: &str,
        folder: &str,
    ) -> Result<Asked, Refusal> {
        if !self.room.can_follow(topics::VAULT) {
            return Err(refusal(
                said_no(
                    "This secret needs the user's approval in the Notchling app, and the app isn't open. Ask the user to open it, then try again.",
                ),
                SecretResult::NoApp,
            ));
        }
        let ticket = self.approvals.open().map_err(|r| match r {
            Refused::Full => refusal(
                failed("Too many uses wait for the user's approval; try again later."),
                SecretResult::Failed,
            ),
            Refused::NoId => refusal(
                ApiError::internal("no randomness for an approval id"),
                SecretResult::Failed,
            ),
        })?;
        let id = ticket.id.clone();
        let names: Vec<&str> = asking.iter().map(|e| e.name.as_str()).collect();
        let entries: Vec<Value> = asking.iter().map(entry_json).collect();
        let verified = crate::vault_caller::verified(self.processes.as_ref(), caller, folder);
        let event = json!({"id": id, "names": names, "entries": entries, "command": command,
                           "folder": folder, "verified": verified});
        self.approvals.describe(&id, &event);
        let raised = self.raise_approval(&id, &names, command, folder);
        self.room
            .publish(topics::VAULT, topics::VAULT_APPROVAL_PENDING, &event);
        let still = || self.switch.is_on() && self.room.can_follow(topics::VAULT);
        let started = Instant::now();
        let decided =
            self.approvals
                .wait(ticket, self.timing.approval_wait, self.timing.slice, &still);
        let (state, answer) = match decided {
            Some(answer) => {
                raised.close(
                    State::Answered,
                    Some(inbox::action_of(answer.value)),
                    Some(answer.by),
                );
                (State::Answered, Some(answer))
            }
            None => {
                let state = self.ended_without_answer(started, self.timing.approval_wait, &still);
                raised.close(state, None, None);
                (state, None)
            }
        };
        let decision = answer.as_ref().map_or(Decision::Deny, |a| a.value);
        self.room.publish(
            topics::VAULT,
            topics::VAULT_APPROVAL_RESOLVED,
            &json!({"id": id, "decision": decision.as_str()}),
        );
        Ok(Asked {
            decision,
            state,
            by: answer.as_ref().map_or(Answerer::NoOne, |a| a.by),
            device: answer.and_then(|a| a.device),
            waited_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    pub(super) fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::the_app(caller)?;
        let bad = || {
            ApiError::bad_request(
                "vault.approval.answer takes {id, decision: \"deny\" | \"once\" | \"always\"}",
            )
        };
        let id = args.get("id").and_then(Value::as_str).ok_or_else(bad)?;
        let decision = args
            .get("decision")
            .and_then(Value::as_str)
            .and_then(Decision::parse)
            .ok_or_else(bad)?;
        Ok(json!({"answered": self.approvals.answer(id, Answered::by_mac(decision))}))
    }

    pub(super) fn request(&self, args: &Value) -> Result<Value, ApiError> {
        let name = text(args, "name", MAX_SHORT)?.unwrap_or_default();
        if !valid_name(&name) {
            return Err(ApiError::bad_request(
                "Give a name: letters, digits, - _ or ., starting with a letter.",
            ));
        }
        let kind = text(args, "kind", MAX_SHORT)?;
        if kind.as_deref().is_some_and(|k| Kind::parse(k).is_none()) {
            return Err(ApiError::bad_request("kind is token or login."));
        }
        let folder = text(args, "folder", MAX_SHORT)?.unwrap_or_default();
        let expires = text(args, "expires", MAX_SHORT)?;
        if expires
            .as_deref()
            .is_some_and(|d| notchling_engine::vault::expiry::parse(d).is_none())
        {
            return Err(ApiError::bad_request(
                "expires takes a day as YYYY-MM-DD, like 2026-12-06.",
            ));
        }
        let request = VaultRequest {
            name,
            kind,
            url: text(args, "url", MAX_SHORT)?,
            note: text(args, "note", MAX_TEXT)?,
            how_to: text(args, "how", MAX_TEXT)?,
            expires,
            reason: text(args, "reason", MAX_SHORT)?,
        };
        if !self.room.can_follow(topics::VAULT) {
            return Err(failed(
                "Notchling isn't open. Ask the user to open it, then try again.",
            ));
        }
        let existing = self
            .store
            .entry(&request.name)
            .map_err(|e| failed(e.to_string()))?;
        let (entry, changes) = match &existing {
            Some(saved) => (
                saved.prefilled(&request),
                PrefillChanges::of(saved, &request),
            ),
            None => (
                VaultEntry::from_request(&request),
                PrefillChanges::default(),
            ),
        };
        let ticket = self.requests.open().map_err(|r| match r {
            Refused::Full => failed("Too many requests wait for the user; try again later."),
            Refused::NoId => ApiError::internal("no randomness for a request id"),
        })?;
        let id = ticket.id.clone();
        let event = json!({"id": id, "request": request, "entry": entry_json(&entry),
                           "existing": existing.is_some(), "changes": changes});
        self.requests.describe(&id, &event);
        let raised = self.raise_request(&id, &request);
        self.room
            .publish(topics::VAULT, topics::VAULT_REQUEST_PENDING, &event);
        let still = || self.switch.is_on() && self.room.can_follow(topics::VAULT);
        let started = Instant::now();
        let answered =
            self.requests
                .wait(ticket, self.timing.request_wait, self.timing.slice, &still);
        let (result, by) = match &answered {
            Some(a) if a.value.is_some() => {
                raised.close(State::Answered, Some(ActionId::Open), Some(a.by));
                (RequestResult::Saved, Some(a.by))
            }
            Some(a) => {
                raised.close(State::Answered, Some(ActionId::Cancel), Some(a.by));
                (RequestResult::Cancelled, Some(a.by))
            }
            None => {
                let state = self.ended_without_answer(started, self.timing.request_wait, &still);
                raised.close(state, None, None);
                match state {
                    State::Expired => (RequestResult::Expired, Some(Answerer::NoOne)),
                    _ => (RequestResult::Withdrawn, Some(Answerer::NoOne)),
                }
            }
        };
        let saved = answered.and_then(|a| a.value);
        self.room.publish(
            topics::VAULT,
            topics::VAULT_REQUEST_RESOLVED,
            &json!({"id": id, "saved": saved.is_some()}),
        );
        let given = self.note_requested(&request.name, &folder, result, by, saved.is_some());
        match saved {
            Some(_) if !given => Err(failed(LOG_DOWN)),
            Some(entry) => Ok(json!({"entry": entry_json(&entry)})),
            None => Err(said_no("The user cancelled.")),
        }
    }

    pub(super) fn cancel(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::the_app(caller)?;
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("vault.request.cancel takes {id}"))?;
        Ok(json!({"answered": self.requests.answer(id, Answered::by_mac(None))}))
    }

    pub(super) fn pending(&self) -> Value {
        json!({"approvals": self.approvals.pending(), "requests": self.requests.pending()})
    }
}
