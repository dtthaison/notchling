//! The vault's waits on the user, for [`super::VaultOps`]: an approval of a use of secrets
//! marked ask-first, and an agent's request for a secret (the form). Each announces itself on the
//! `vault` topic and is kept until it ends, so `vault.pending` can show it to an app that
//! connects later.

use super::*;
use notchling_engine::inbox::{ActionId, State};
use std::time::Instant;

impl VaultOps {
    /// Asks the app and waits. With no app following the vault, a no at once.
    pub(super) fn approve(
        &self,
        caller: &Caller,
        asking: &[VaultEntry],
        command: &str,
        folder: &str,
    ) -> Result<Decision, ApiError> {
        if !self.room.can_follow(topics::VAULT) {
            return Err(said_no(
                "This secret needs the user's approval in the Notchling app, and the app isn't open. Ask the user to open it, then try again.",
            ));
        }
        let ticket = self.approvals.open().map_err(|r| match r {
            Refused::Full => failed("Too many uses wait for the user's approval; try again later."),
            Refused::NoId => ApiError::internal("no randomness for an approval id"),
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
        match decided {
            Some(decision) => raised.close(State::Answered, Some(inbox::action_of(decision))),
            None => {
                let state = self.ended_without_answer(started, self.timing.approval_wait, &still);
                raised.close(state, None);
            }
        }
        let decision = decided.unwrap_or(Decision::Deny);
        self.room.publish(
            topics::VAULT,
            topics::VAULT_APPROVAL_RESOLVED,
            &json!({"id": id, "decision": decision.as_str()}),
        );
        Ok(decision)
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
        Ok(json!({"answered": self.approvals.answer(id, decision)}))
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
        match &answered {
            Some(Some(_)) => raised.close(State::Answered, Some(ActionId::Open)),
            Some(None) => raised.close(State::Answered, Some(ActionId::Cancel)),
            None => {
                let state = self.ended_without_answer(started, self.timing.request_wait, &still);
                raised.close(state, None);
            }
        }
        let saved = answered.flatten();
        self.room.publish(
            topics::VAULT,
            topics::VAULT_REQUEST_RESOLVED,
            &json!({"id": id, "saved": saved.is_some()}),
        );
        match saved {
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
        Ok(json!({"answered": self.requests.answer(id, None)}))
    }

    /// What waits for the user now, oldest first: the events that announced each.
    pub(super) fn pending(&self) -> Value {
        json!({"approvals": self.approvals.pending(), "requests": self.requests.pending()})
    }
}
