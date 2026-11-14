use self::vault_events::{Trail, LOG_DOWN};
use crate::acting_ops::Room;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::op_group::OpGroup;
pub use crate::switches::{SwitchFlag, VAULT_OFF};
use crate::vault_args::{entry_from, entry_json, text, value_from, Decision};
use crate::vault_caller::{Processes, SystemProcesses};
use crate::vault_wait::{Answered, OffHook, Refused, Waiters};
use notchling_engine::inbox::State;
use notchling_engine::vault::list::{self, label};
use notchling_engine::vault::{
    now_iso, system_keychain, valid_name, Kind, PrefillChanges, Unsupported, VaultEntry,
    VaultKeychain, VaultRequest, VaultStore,
};
use notchling_events::kinds::choices::Answerer;
use notchling_events::kinds::secrets::{Approval, EntryAction, RequestResult, SecretResult};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

pub const APPROVAL_WAIT: Duration = Duration::from_secs(300);
pub const REQUEST_WAIT: Duration = Duration::from_secs(540);
pub const MAX_APPROVALS: usize = 8;
pub const MAX_REQUESTS: usize = 4;

const MAX_NAMES: usize = 16;
const MAX_COMMAND: usize = 64 * 1024;
const MAX_TEXT: usize = 8 * 1024;
const MAX_SHORT: usize = 2 * 1024;

pub const VAULT_OPS: [&str; 9] = [
    ops::VAULT_FIND,
    ops::VAULT_USE,
    ops::VAULT_REQUEST,
    ops::VAULT_LIST,
    ops::VAULT_SAVE,
    ops::VAULT_REMOVE,
    ops::VAULT_APPROVAL_ANSWER,
    ops::VAULT_REQUEST_CANCEL,
    ops::VAULT_PENDING,
];

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

pub struct VaultOps {
    switch: Arc<SwitchFlag>,
    store: Arc<VaultStore>,
    room: Arc<dyn Room>,
    events: Option<Events>,
    approvals: Arc<Waiters<Answered<Decision>>>,
    requests: Arc<Waiters<Answered<Option<VaultEntry>>>>,
    timing: VaultTiming,
    processes: Arc<dyn Processes>,
    inbox: Option<Arc<InboxOps>>,
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
            events: None,
            approvals: Arc::new(Waiters::new("vault-approval-", MAX_APPROVALS)),
            requests: Arc::new(Waiters::new("vault-request-", MAX_REQUESTS)),
            timing: VaultTiming::default(),
            processes: Arc::new(SystemProcesses),
            inbox: None,
        };
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

    pub fn for_daemon(
        layout: &Layout,
        switch: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
        inbox: Arc<InboxOps>,
        events: Events,
    ) -> VaultOps {
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
        .with_events(events)
        .with_inbox(inbox)
    }

    pub fn with_inbox(mut self, inbox: Arc<InboxOps>) -> VaultOps {
        self.inbox = Some(inbox);
        self
    }

    pub fn with_events(mut self, events: Events) -> VaultOps {
        self.events = Some(events);
        self
    }

    pub fn with_timing(mut self, timing: VaultTiming) -> VaultOps {
        self.timing = timing;
        self
    }

    pub fn with_processes(mut self, processes: Arc<dyn Processes>) -> VaultOps {
        self.processes = processes;
        self
    }

    pub fn is_on(&self) -> bool {
        self.switch.is_on()
    }

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
            self.off_hook().run();
            return Err(self.switch.refusal());
        }
        match op {
            ops::VAULT_FIND => self.find(args),
            ops::VAULT_USE => self.use_secrets(caller, args),
            ops::VAULT_REQUEST => self.request(args),
            ops::VAULT_LIST => self.entries().map(|e| json!({"entries": e})),
            ops::VAULT_SAVE => self.save(caller, args),
            ops::VAULT_REMOVE => self.remove(args),
            ops::VAULT_APPROVAL_ANSWER => self.answer(caller, args),
            ops::VAULT_REQUEST_CANCEL => self.cancel(caller, args),
            ops::VAULT_PENDING => Ok(self.pending()),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    pub fn close(&self) {
        self.approvals.drain();
        self.requests.drain();
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
        let mut trail = Trail::new(
            text(args, "folder", MAX_SHORT)
                .ok()
                .flatten()
                .unwrap_or_default(),
            text(args, "command", MAX_COMMAND)
                .ok()
                .flatten()
                .unwrap_or_default(),
        );
        match self.use_inner(caller, args, &mut trail) {
            Ok(reply) => {
                if self.note_used(&trail, SecretResult::Given, true) {
                    Ok(reply)
                } else {
                    Err(failed(LOG_DOWN))
                }
            }
            Err((e, result)) => {
                if !trail.names.is_empty() {
                    self.note_used(&trail, result, false);
                }
                Err(e)
            }
        }
    }

    fn use_inner(
        &self,
        caller: &Caller,
        args: &Value,
        trail: &mut Trail,
    ) -> Result<Value, (ApiError, SecretResult)> {
        let none = |e: ApiError| (e, SecretResult::Failed);
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
        trail.names = picked.iter().map(|e| e.name.clone()).collect();
        let fail = |e: ApiError| (e, SecretResult::Failed);
        let asking: Vec<VaultEntry> = picked.iter().filter(|e| e.asks()).cloned().collect();
        if !asking.is_empty() {
            let asked = self
                .approve(caller, &asking, &command, &folder)
                .map_err(|r| (r.err, r.result))?;
            trail.approval = Approval::Asked;
            trail.by = Some(asked.by);
            trail.device = asked.device.clone();
            trail.waited_ms = Some(asked.waited_ms);
            if !self.switch.is_on() {
                return Err((self.switch.refusal(), SecretResult::Cancelled));
            }
            match asked.decision {
                Decision::Deny => {
                    let why = match asked.state {
                        State::Answered => SecretResult::Denied,
                        State::Expired => SecretResult::Expired,
                        _ => SecretResult::Cancelled,
                    };
                    return Err((said_no("The user didn't allow it."), why));
                }
                Decision::Once => {}
                Decision::Always => {
                    let ids: BTreeSet<String> = asking.iter().map(VaultEntry::id).collect();
                    self.store
                        .stop_asking(&ids)
                        .map_err(|e| fail(failed(e.to_string())))?;
                    let names: Vec<&str> = asking.iter().map(|e| e.name.as_str()).collect();
                    self.publish_changed(&names, false);
                    for name in &names {
                        self.note_entry(name, EntryAction::AskFirstOff);
                    }
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
        if !self.switch.is_on() {
            return Err((self.switch.refusal(), SecretResult::Cancelled));
        }
        let (env, hide) = list::environment(&pairs);
        Ok(json!({"env": env, "hide": hide}))
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
        self.note_saved(old.as_ref(), &saved);
        let answered = request.as_deref().is_some_and(|id| {
            self.requests
                .answer(id, Answered::by_mac(Some(saved.entry.clone())))
        });
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
            self.note_entry(&name, EntryAction::Removed);
        }
        Ok(json!({"removed": removed}))
    }

    fn note_saved(&self, old: Option<&VaultEntry>, saved: &notchling_engine::vault::Saved) {
        let flag_only = old.is_some_and(|old| {
            let mut same = old.clone();
            same.ask = saved.entry.ask;
            same.updated = saved.entry.updated.clone();
            same == saved.entry && !saved.value_changed && old.asks() != saved.entry.asks()
        });
        let action = match (flag_only, saved.entry.asks()) {
            (true, true) => EntryAction::AskFirstOn,
            (true, false) => EntryAction::AskFirstOff,
            _ => EntryAction::Saved,
        };
        self.note_entry(&saved.entry.name, action);
        if let Some(old) = old.filter(|o| o.id() != saved.entry.id()) {
            self.note_entry(&old.name, EntryAction::Removed);
        }
    }

    fn publish_changed(&self, names: &[&str], value_changed: bool) {
        let mut data = Map::new();
        data.insert("names".into(), json!(names));
        data.insert("value_changed".into(), json!(value_changed));
        self.room
            .publish(topics::VAULT, topics::VAULT_CHANGED, &Value::Object(data));
    }
}

#[path = "vault_asks.rs"]
mod asks;
#[path = "vault_inbox.rs"]
mod inbox;
#[path = "vault_events.rs"]
mod vault_events;

#[cfg(test)]
#[path = "vault_ops_events_tests.rs"]
mod events_tests;
#[cfg(test)]
#[path = "vault_ops_pending_tests.rs"]
mod pending_tests;
#[cfg(test)]
#[path = "vault_ops_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "vault_ops_inbox_tests.rs"]
mod vault_ops_inbox_tests;

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

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::VAULT).then_some(Tier::App)
    }
}
