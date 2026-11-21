use super::*;
use notchling_engine::inbox::State;
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
        who: &Who<'_>,
        asking: &[VaultEntry],
        command: &str,
        folder: &str,
    ) -> Result<Asked, Refusal> {
        if !self.room.can_follow(topics::VAULT) {
            return Err(refusal(
                said_no(
                    "This secret needs the user's approval in the Notchling app, and the app isn't open. Ask the user to open it, then try again.",
                )
                .with_reason("no_app"),
                SecretResult::NoApp,
            ));
        }
        let ticket = self.approvals.open().map_err(|r| match r {
            Refused::Full => refusal(
                failed("Too many uses wait for the user's approval; try again later.")
                    .with_reason("busy"),
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
        let verified = match who {
            Who::Caller(caller) => {
                crate::vault_caller::verified(self.processes.as_ref(), caller, folder)
            }
            Who::Grant(grant) => json!({"node": grant.node_name, "job": grant.job}),
            Who::Machine(_) => Value::Null,
        };
        let mut event = json!({"id": id, "names": names, "entries": entries, "command": command,
                           "folder": folder, "verified": verified});
        if let Who::Machine(machine) = who {
            event["machine"] = json!(machine);
        }
        if let Some(session) = match who {
            Who::Caller(caller) => self.trusted_session(caller),
            Who::Grant(_) | Who::Machine(_) => None,
        } {
            event["session"] = json!(session);
        }
        self.approvals.describe(&id, &event);
        let by = match who {
            Who::Grant(grant) => Some(*grant),
            Who::Caller(_) | Who::Machine(_) => None,
        };
        let raised = self.raise_approval(&id, &names, (command, folder), by);
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

    pub(super) fn request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let dry = args.get("dryRun").and_then(Value::as_bool) == Some(true);
        match self.request_inner(caller, args, dry) {
            Err(e) if dry => Ok(json!({
                "dryRun": true, "op": ops::VAULT_REQUEST, "would": "refuse",
                "change": {"name": args.get("name")}, "request": null,
                "error": crate::contract::Failure::from_api(Some(ops::VAULT_REQUEST), e)
                    .to_value()["error"].clone()})),
            other => other,
        }
    }

    fn request_inner(&self, caller: &Caller, args: &Value, dry: bool) -> Result<Value, ApiError> {
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
        let consent = self
            .consent
            .get()
            .ok_or_else(|| ApiError::internal("the vault has no consent ops"))?;
        let (reason, expires_in_ms, key) = options(args);
        let request = VaultRequest {
            name,
            kind,
            url: text(args, "url", MAX_SHORT)?,
            note: text(args, "note", MAX_TEXT)?,
            how_to: text(args, "how", MAX_TEXT)?,
            expires,
            reason: None,
        };
        let vault_on = self.switch.is_on();
        let existing = if vault_on {
            self.store
                .entry(&request.name)
                .map_err(|e| failed(e.to_string()))?
        } else {
            None
        };
        let known = if vault_on {
            json!(existing.is_some())
        } else {
            Value::Null
        };
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
        let folder = consent.who(caller).folder.unwrap_or_default();
        let verified = crate::vault_caller::verified(self.processes.as_ref(), caller, &folder);
        let opening = Opening {
            kind: SECRET_REQUEST,
            target: request.name.clone(),
            change: json!({"name": request.name, "folder": folder, "request": request,
                           "entry": entry_json(&entry), "existing": known,
                           "revision": existing.as_ref().map(|e| e.updated.clone()),
                           "changes": changes, "verified": verified}),
            facts: json!([{"line": "consent-secret-request",
                           "args": {"name": request.name, "update": known}}]),
            request: json!({"op": ops::VAULT_REQUEST, "request": request}),
            reason,
            expires_in_ms,
            key,
        };
        if dry {
            let would = consent.would(&opening)?;
            return Ok(
                json!({"dryRun": true, "op": ops::VAULT_REQUEST, "would": "ask",
                             "change": {"name": request.name, "update": known},
                             "request": would, "error": null}),
            );
        }
        let opened = consent.open(caller, opening)?;
        let row = opened.row().clone();
        if matches!(opened, notchling_engine::consent::Opened::Fresh(_)) && self.switch.is_on() {
            self.room.publish(
                topics::VAULT,
                topics::VAULT_REQUEST_PENDING,
                &self.request_event(&row),
            );
        }
        Ok(json!({"state": row.state.as_str(), "request": consent.project(&row, true)}))
    }

    pub(super) fn cancel(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::the_app(caller)?;
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("vault.request.cancel takes {id}"))?;
        let answered = self.consent.get().is_some_and(|c| c.deny_by_app(id));
        Ok(json!({"answered": answered}))
    }

    pub(super) fn pending(&self) -> Value {
        let now = self.consent.get().map_or(0, |c| c.now());
        let requests: Vec<Value> = self
            .consent
            .get()
            .map(|c| c.store().open_rows().unwrap_or_default())
            .unwrap_or_default()
            .iter()
            .filter(|r| r.kind == SECRET_REQUEST && r.state.open() && r.expires_ms > now)
            .map(|r| self.request_event(r))
            .collect();
        json!({"approvals": self.approvals.pending(), "requests": requests})
    }

    pub(super) fn request_event(&self, row: &notchling_engine::consent::Consent) -> Value {
        let mut request = row.change.get("request").cloned().unwrap_or(Value::Null);
        if let (Some(reason), Some(map)) = (&row.reason, request.as_object_mut()) {
            map.insert("reason".into(), json!(reason));
        }
        let unknown = row.change.get("existing").is_none_or(Value::is_null);
        let live = if unknown && self.switch.is_on() {
            self.live_prefill(&request)
        } else {
            None
        };
        let (entry, existing, changes) = live.unwrap_or_else(|| {
            (
                row.change.get("entry").cloned().unwrap_or(Value::Null),
                row.change.get("existing").cloned().unwrap_or(Value::Null),
                row.change.get("changes").cloned().unwrap_or(Value::Null),
            )
        });
        let mut event = json!({"id": row.id, "request": request,
            "entry": entry, "existing": existing, "changes": changes,
            "verified": row.change.get("verified")});
        if let Some(session) = &row.session {
            event["session"] = json!(session);
        }
        event
    }

    fn live_prefill(&self, request: &Value) -> Option<(Value, Value, Value)> {
        let request: VaultRequest = serde_json::from_value(request.clone()).ok()?;
        let saved = self.store.entry(&request.name).ok()?;
        let (entry, changes) = match &saved {
            Some(saved) => (
                saved.prefilled(&request),
                PrefillChanges::of(saved, &request),
            ),
            None => (
                VaultEntry::from_request(&request),
                PrefillChanges::default(),
            ),
        };
        Some((
            entry_json(&entry),
            json!(saved.is_some()),
            serde_json::to_value(changes).ok()?,
        ))
    }
}

pub(super) struct SecretHandler {
    pub(super) ops: std::sync::Weak<VaultOps>,
}

impl crate::consent_ops::Handler for SecretHandler {
    fn shape(&self, _row: &notchling_engine::consent::Consent, view: &mut Value, agent: bool) {
        let Some(ops) = self.ops.upgrade() else {
            return;
        };
        let on = ops.switch.is_on();
        let facts = view.get_mut("facts").and_then(Value::as_array_mut);
        for fact in facts.into_iter().flatten() {
            if let Some(args) = fact.get_mut("args").and_then(Value::as_object_mut) {
                args.insert("vaultOn".into(), json!(on));
            }
        }
        if agent {
            if let Some(change) = view.get_mut("change").and_then(Value::as_object_mut) {
                for key in ["entry", "existing", "changes", "revision", "verified"] {
                    change.remove(key);
                }
            }
        }
    }

    fn ended(
        &self,
        row: &notchling_engine::consent::Consent,
        then: notchling_events::kinds::choices::ConsentThen,
        _device: Option<&str>,
    ) {
        use notchling_events::kinds::choices::ConsentThen;
        let Some(ops) = self.ops.upgrade() else {
            return;
        };
        let (result, saved) = match then {
            ConsentThen::Applied => (RequestResult::Saved, true),
            ConsentThen::Denied | ConsentThen::Cancelled => (RequestResult::Cancelled, false),
            ConsentThen::Expired => (RequestResult::Expired, false),
            _ => (RequestResult::Withdrawn, false),
        };
        let folder = row
            .change
            .get("folder")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let by = row.by.as_deref().and_then(Answerer::from_name);
        ops.note_requested(&row.target, folder, result, by, saved);
        ops.room.publish(
            topics::VAULT,
            topics::VAULT_REQUEST_RESOLVED,
            &json!({"id": row.id, "saved": saved}),
        );
    }
}
