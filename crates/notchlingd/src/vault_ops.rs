//! E9 in the daemon: the Secret Vault as a service of its own, behind the `vault` switch
//! (decision 0007: the vault is a separate service inside the daemon, with its own checks).
//!
//! While the switch is off (every start, until the app's `settings.set_switch`), every op below
//! answers `forbidden` with [`VAULT_OFF`], nothing reads the Keychain item or the list, values
//! read earlier are dropped, and no socket beyond the daemon's own is bound.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `vault.find` | agent | `{query?}` | `{entries: [Entry], saved: ["github (api.github.com)", …]}` |
//! | `vault.use` | agent | `{names: [..], folder, command}` | `{env: {VAR: value}, hide: {value: "[secret:name]"}}` |
//! | `vault.request` | agent | `{name, kind?, url?, note?, how?, expires?, reason?}` | `{entry: Entry}` once saved |
//! | `vault.list` | app | `{}` | `{entries: [Entry]}` |
//! | `vault.save` | app | `{entry: Entry, value?: {username?, secret}, replacing?: name, request?: id}` | `{entry, answered}` |
//! | `vault.remove` | app | `{name}` | `{removed}` |
//! | `vault.approval.answer` | app | `{id, decision: "deny"\|"once"\|"always"}` | `{answered}` |
//! | `vault.request.cancel` | app | `{id}` | `{answered}` |
//!
//! `Entry` is `vault.json`'s shape: `{name, kind, note, url?, howTo?, expires?, userVar,
//! secretVar, ask?, updated}`. No reply but `vault.use`'s carries a value, and no event ever does.
//!
//! **Approval.** `vault.use` of an entry marked ask-first publishes `vault.approval.pending {id,
//! names, entries, command, folder}` on the `vault` topic, which only the app tier may follow, and
//! waits up to [`APPROVAL_WAIT`] for `vault.approval.answer`. The id is 128 random bits. The
//! answer must come from an app-tier client that said hello as the app; an agent-tier client is
//! refused by the server before this module sees the call. With no app following `vault` the use
//! is denied at once, and a wait ends denied when the app leaves, the switch goes off or the
//! daemon stops. Always Allow turns ask-first off for those entries. A denial is `needs_approval`
//! ("The user didn't allow it."), which `notchling secret` exits 3 on.
//!
//! **Requests.** `vault.request` publishes `vault.request.pending {id, request, entry, existing,
//! changes}` (the form, filled in: an existing entry with the passed fields laid over it, and what
//! that changed) and waits up to [`REQUEST_WAIT`] for the app's `vault.save {…, request: id}` or
//! `vault.request.cancel {id}`. A cancel is `needs_approval` ("The user cancelled.").
//!
//! Errors carry names and fixed texts only: never a value, never a parse error that could quote
//! one. Agent-tier calls are written to the audit log here (op, tier, peer, outcome and the
//! names used); the server logs every app-tier call.

use crate::acting_ops::Room;
use crate::layout::Layout;
use crate::op_group::OpGroup;
pub use crate::switches::{SwitchFlag, VAULT_OFF};
use crate::vault_args::{entry_from, entry_json, text, value_from, Decision};
use crate::vault_wait::{OffHook, Refused, Waiters};
use notchling_engine::vault::list::{self, label};
use notchling_engine::vault::{
    now_iso, system_keychain, valid_name, Kind, PrefillChanges, Unsupported, VaultEntry,
    VaultKeychain, VaultRequest, VaultStore,
};
use notchling_local_api::{
    ops, topics, ApiError, AuditEntry, AuditLog, Caller, ClientKind, ErrorCode, OpSpec, Tier,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

/// How long a use waits for the user's approval, and a request for the form.
pub const APPROVAL_WAIT: Duration = Duration::from_secs(300);
pub const REQUEST_WAIT: Duration = Duration::from_secs(540);
/// How many approvals, and requests, may wait at once.
pub const MAX_APPROVALS: usize = 8;
pub const MAX_REQUESTS: usize = 4;

/// Caps on what an agent sends, so an event stays small.
const MAX_NAMES: usize = 16;
const MAX_COMMAND: usize = 64 * 1024;
const MAX_TEXT: usize = 8 * 1024;
const MAX_SHORT: usize = 2 * 1024;

/// Every vault op.
pub const VAULT_OPS: [&str; 8] = [
    ops::VAULT_FIND,
    ops::VAULT_USE,
    ops::VAULT_REQUEST,
    ops::VAULT_LIST,
    ops::VAULT_SAVE,
    ops::VAULT_REMOVE,
    ops::VAULT_APPROVAL_ANSWER,
    ops::VAULT_REQUEST_CANCEL,
];

/// How long waits last and how often they look up; tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct VaultTiming {
    pub approval_wait: Duration,
    pub request_wait: Duration,
    pub slice: Duration,
}

impl Default for VaultTiming {
    fn default() -> Self {
        VaultTiming {
            approval_wait: APPROVAL_WAIT,
            request_wait: REQUEST_WAIT,
            slice: Duration::from_millis(250),
        }
    }
}

/// The vault service.
pub struct VaultOps {
    switch: Arc<SwitchFlag>,
    store: Arc<VaultStore>,
    room: Arc<dyn Room>,
    audit: Option<AuditLog>,
    approvals: Arc<Waiters<Decision>>,
    requests: Arc<Waiters<Option<VaultEntry>>>,
    timing: VaultTiming,
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn said_no(message: &str) -> ApiError {
    ApiError::new(ErrorCode::NeedsApproval, message)
}

impl VaultOps {
    pub fn new(switch: Arc<SwitchFlag>, store: VaultStore, room: Arc<dyn Room>) -> VaultOps {
        let ops = VaultOps {
            switch,
            store: Arc::new(store),
            room,
            audit: None,
            approvals: Arc::new(Waiters::new("vault-approval-", MAX_APPROVALS)),
            requests: Arc::new(Waiters::new("vault-request-", MAX_REQUESTS)),
            timing: VaultTiming::default(),
        };
        // Off forgets at once, not at the next call: a value must not sit in memory, nor an agent
        // wait for an approval, after the user turned the vault off.
        let hook = ops.off_hook();
        ops.switch.subscribe(move |on| {
            if !on {
                hook.run();
            }
        });
        ops
    }

    fn off_hook(&self) -> OffHook {
        OffHook {
            store: Arc::downgrade(&self.store),
            approvals: Arc::downgrade(&self.approvals),
            requests: Arc::downgrade(&self.requests),
        }
    }

    /// The daemon's: its own `vault.json` ([`Layout::vault_index`]), the system's Keychain item
    /// (never under a fixture root, whatever the switch says) and its audit log.
    pub fn for_daemon(layout: &Layout, switch: Arc<SwitchFlag>, room: Arc<dyn Room>) -> VaultOps {
        let keychain: Box<dyn VaultKeychain> = if layout.fixture {
            Box::new(Unsupported(
                "a daemon over a fixture root reads no Keychain",
            ))
        } else {
            system_keychain()
        };
        VaultOps::new(
            switch,
            VaultStore::new(layout.vault_index(), keychain),
            room,
        )
        .with_audit(AuditLog::new(layout.audit_path()))
    }

    /// Writes agent-tier calls to this log.
    pub fn with_audit(mut self, audit: AuditLog) -> VaultOps {
        self.audit = Some(audit);
        self
    }

    pub fn with_timing(mut self, timing: VaultTiming) -> VaultOps {
        self.timing = timing;
        self
    }

    pub fn is_on(&self) -> bool {
        self.switch.is_on()
    }

    /// The names the `notchling-vault` skill's description lists (`github (api.github.com)`),
    /// read from `vault.json` only while the vault is on: none while off, when nothing reads it.
    pub fn skill_names(&self) -> Vec<String> {
        if !self.switch.is_on() {
            return Vec::new();
        }
        self.store
            .entries()
            .map(|entries| notchling_engine::vault::skill::names(&entries))
            .unwrap_or_default()
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::VAULT_FIND | ops::VAULT_USE | ops::VAULT_REQUEST => Some(OpSpec::AGENT),
            op if VAULT_OPS.contains(&op) => Some(OpSpec::APP),
            _ => None,
        }
    }

    pub fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if !self.switch.is_on() {
            // Values read while it was on don't outlive it (the switch's listener did this too).
            self.off_hook().run();
            self.audit_agent(caller, op, "off");
            return Err(self.switch.refusal());
        }
        let result = match op {
            ops::VAULT_FIND => self.find(args),
            ops::VAULT_USE => self.use_secrets(caller, args),
            ops::VAULT_REQUEST => self.request(args),
            ops::VAULT_LIST => self.entries().map(|e| json!({"entries": e})),
            ops::VAULT_SAVE => self.save(caller, args),
            ops::VAULT_REMOVE => self.remove(args),
            ops::VAULT_APPROVAL_ANSWER => self.answer(caller, args),
            ops::VAULT_REQUEST_CANCEL => self.cancel(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        };
        if op != ops::VAULT_USE {
            let outcome = match &result {
                Ok(_) => "ok",
                Err(e) => e.code.as_str(),
            };
            self.audit_agent(caller, op, outcome);
        }
        result
    }

    /// The daemon stops: every wait ends with a no.
    pub fn close(&self) {
        self.approvals.drain();
        self.requests.drain();
    }

    fn audit_agent(&self, caller: &Caller, op: &str, outcome: &str) {
        if caller.tier != Tier::Agent {
            return;
        }
        if let Some(log) = &self.audit {
            let entry = AuditEntry::now(op, Some(caller.tier), caller.peer.as_deref(), outcome);
            let _ = log.append(&entry);
        }
    }

    fn entries(&self) -> Result<Vec<VaultEntry>, ApiError> {
        self.store.entries().map_err(|e| failed(e.to_string()))
    }

    fn find(&self, args: &Value) -> Result<Value, ApiError> {
        let query = text(args, "query", MAX_SHORT)?.unwrap_or_default();
        let all = self.entries()?;
        let saved: Vec<String> = all.iter().map(label).collect();
        Ok(json!({"entries": list::find(&query, &all), "saved": saved}))
    }

    /// Only the app, as the app, answers what it was asked: not a command line run from its file.
    fn the_app(caller: &Caller) -> Result<(), ApiError> {
        if caller.tier == Tier::App && caller.client == ClientKind::App {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app may answer the vault",
            ))
        }
    }

    fn use_secrets(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let result = self.use_inner(args);
        let outcome = match &result {
            Ok((_, names)) => format!("ok:{}", names.join(",")),
            Err((e, names)) if e.code == ErrorCode::NeedsApproval => {
                format!("denied:{}", names.join(","))
            }
            Err((e, _)) => e.code.as_str().to_string(),
        };
        self.audit_agent(caller, ops::VAULT_USE, &outcome);
        result.map(|(v, _)| v).map_err(|(e, _)| e)
    }

    /// The reply and the names used (for the audit log; only names that exist).
    #[allow(clippy::type_complexity)]
    fn use_inner(&self, args: &Value) -> Result<(Value, Vec<String>), (ApiError, Vec<String>)> {
        let none = |e: ApiError| (e, Vec::new());
        let bad = || ApiError::bad_request("vault.use takes {names: [name, …], folder, command}");
        let names: Vec<String> = match args.get("names").and_then(Value::as_array) {
            Some(list) if !list.is_empty() && list.len() <= MAX_NAMES => list
                .iter()
                .map(|n| n.as_str().map(str::to_string).ok_or_else(bad))
                .collect::<Result<_, _>>()
                .map_err(none)?,
            _ => return Err(none(bad())),
        };
        let folder = text(args, "folder", MAX_SHORT)
            .map_err(none)?
            .unwrap_or_default();
        let command = text(args, "command", MAX_COMMAND)
            .map_err(none)?
            .unwrap_or_default();
        let all = self.entries().map_err(none)?;
        let mut picked: Vec<VaultEntry> = Vec::new();
        for name in &names {
            let id = name.to_lowercase();
            match all.iter().find(|e| e.id() == id) {
                Some(e) if !picked.iter().any(|p| p.id() == id) => picked.push(e.clone()),
                Some(_) => {}
                None => {
                    let saved = if all.is_empty() {
                        "none".to_string()
                    } else {
                        all.iter()
                            .map(|e| e.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    // A name that isn't one could be anything an agent pasted: not repeated.
                    let which = if valid_name(name) {
                        format!("No secret named {name}.")
                    } else {
                        "No secret by that name.".to_string()
                    };
                    return Err(none(failed(format!(
                        "{which} Saved: {saved}. Ask the user for it with secret-request."
                    ))));
                }
            }
        }
        let used: Vec<String> = picked.iter().map(|e| e.name.clone()).collect();
        let fail = |e: ApiError| (e, used.clone());
        let asking: Vec<VaultEntry> = picked.iter().filter(|e| e.asks()).cloned().collect();
        if !asking.is_empty() {
            let decision = self.approve(&asking, &command, &folder).map_err(fail)?;
            // The switch may have gone off while the user looked.
            if !self.switch.is_on() {
                return Err(fail(self.switch.refusal()));
            }
            match decision {
                Decision::Deny => return Err(fail(said_no("The user didn't allow it."))),
                Decision::Once => {}
                Decision::Always => {
                    let ids: BTreeSet<String> = asking.iter().map(VaultEntry::id).collect();
                    self.store
                        .stop_asking(&ids)
                        .map_err(|e| fail(failed(e.to_string())))?;
                    let names: Vec<&str> = asking.iter().map(|e| e.name.as_str()).collect();
                    self.publish_changed(&names, false);
                }
            }
        }
        let mut pairs = Vec::new();
        for entry in picked {
            match self.store.value(&entry) {
                Ok(Some(value)) => pairs.push((entry, value)),
                Ok(None) => {
                    return Err(fail(failed(format!(
                        "{} has no value saved. Ask the user to update it with secret-request {}.",
                        entry.name, entry.name
                    ))))
                }
                Err(e) => return Err(fail(failed(e.to_string()))),
            }
        }
        let (env, hide) = list::environment(&pairs);
        Ok((json!({"env": env, "hide": hide}), used))
    }

    /// Asks the app and waits. With no app following the vault, a no at once.
    fn approve(
        &self,
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
        self.room.publish(
            topics::VAULT,
            topics::VAULT_APPROVAL_PENDING,
            &json!({"id": id, "names": names, "entries": entries, "command": command, "folder": folder}),
        );
        let still = || self.switch.is_on() && self.room.can_follow(topics::VAULT);
        let decision = self
            .approvals
            .wait(ticket, self.timing.approval_wait, self.timing.slice, &still)
            .unwrap_or(Decision::Deny);
        self.room.publish(
            topics::VAULT,
            topics::VAULT_APPROVAL_RESOLVED,
            &json!({"id": id, "decision": decision.as_str()}),
        );
        Ok(decision)
    }

    fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
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

    fn request(&self, args: &Value) -> Result<Value, ApiError> {
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
        self.room.publish(
            topics::VAULT,
            topics::VAULT_REQUEST_PENDING,
            &json!({"id": id, "request": request, "entry": entry_json(&entry),
                    "existing": existing.is_some(), "changes": changes}),
        );
        let still = || self.switch.is_on() && self.room.can_follow(topics::VAULT);
        let saved = self
            .requests
            .wait(ticket, self.timing.request_wait, self.timing.slice, &still)
            .flatten();
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

    fn cancel(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        Self::the_app(caller)?;
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("vault.request.cancel takes {id}"))?;
        Ok(json!({"answered": self.requests.answer(id, None)}))
    }

    fn save(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let request = text(args, "request", MAX_SHORT)?;
        if request.is_some() {
            Self::the_app(caller)?;
        }
        let entry = entry_from(args.get("entry"))?;
        let value = value_from(args.get("value"))?;
        let old = match text(args, "replacing", MAX_SHORT)? {
            Some(name) => Some(
                self.store
                    .entry(&name)
                    .map_err(|e| failed(e.to_string()))?
                    .ok_or_else(|| failed(format!("There's no secret named {name}.")))?,
            ),
            None => None,
        };
        let saved = self
            .store
            .save(&entry, value, old.as_ref(), &now_iso())
            .map_err(|e| failed(e.to_string()))?;
        let mut names = vec![saved.entry.name.as_str()];
        if let Some(old) = old.as_ref().filter(|o| o.id() != saved.entry.id()) {
            names.push(old.name.as_str());
        }
        self.publish_changed(&names, saved.value_changed);
        let answered = request
            .as_deref()
            .is_some_and(|id| self.requests.answer(id, Some(saved.entry.clone())));
        Ok(json!({"entry": entry_json(&saved.entry), "answered": answered}))
    }

    fn remove(&self, args: &Value) -> Result<Value, ApiError> {
        let name = text(args, "name", MAX_SHORT)?
            .ok_or_else(|| ApiError::bad_request("vault.remove takes {name}"))?;
        let removed = self
            .store
            .remove(&name)
            .map_err(|e| failed(e.to_string()))?;
        if removed {
            self.publish_changed(&[name.as_str()], true);
        }
        Ok(json!({"removed": removed}))
    }

    fn publish_changed(&self, names: &[&str], value_changed: bool) {
        let mut data = Map::new();
        data.insert("names".into(), json!(names));
        data.insert("value_changed".into(), json!(value_changed));
        self.room
            .publish(topics::VAULT, topics::VAULT_CHANGED, &Value::Object(data));
    }
}

#[cfg(test)]
#[path = "vault_ops_tests.rs"]
mod tests;

impl OpGroup for VaultOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        VaultOps::spec(self, op)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        VaultOps::call(self, caller, op, args)
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::VAULT]
    }

    /// The approvals and forms are the app's: their ids are what answers them.
    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::VAULT).then_some(Tier::App)
    }
}
