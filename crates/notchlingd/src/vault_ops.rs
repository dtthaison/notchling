use self::release::Who;
pub use self::release::{was_denied, Looked, ReleaseFor, Released, Still, DENIED};
use self::vault_events::{Trail, LOG_DOWN};
use crate::acting_ops::Room;
use crate::consent_ops::{options, ConsentOps, Opening};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::op_group::OpGroup;
pub use crate::switches::{SwitchFlag, VAULT_OFF};
use crate::vault_args::{entry_from, entry_json, text, value_from, Decision};
use crate::vault_caller::{Processes, SystemProcesses};
use crate::vault_wait::{Answered, OffHook, Refused, Waiters};
use notchling_engine::consent::SECRET_REQUEST;
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
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const APPROVAL_WAIT: Duration = Duration::from_secs(300);
pub const MAX_APPROVALS: usize = 8;

const MAX_NAMES: usize = 16;
const MAX_COMMAND: usize = 64 * 1024;
const MAX_TEXT: usize = 8 * 1024;
const MAX_SHORT: usize = 2 * 1024;

pub const VAULT_OPS: [&str; 10] = [
    ops::VAULT_FIND,
    ops::VAULT_USE,
    ops::VAULT_REQUEST,
    ops::VAULT_LIST,
    ops::VAULT_SAVE,
    ops::VAULT_REMOVE,
    ops::VAULT_APPROVAL_ANSWER,
    ops::VAULT_REQUEST_CANCEL,
    ops::VAULT_PENDING,
    ops::VAULT_ADMIN,
];

#[derive(Clone, Copy, Debug)]
pub struct VaultTiming {
    pub approval_wait: Duration,
    pub slice: Duration,
}

impl Default for VaultTiming {
    fn default() -> Self {
        VaultTiming {
            approval_wait: APPROVAL_WAIT,
            slice: Duration::from_millis(250),
        }
    }
}

pub trait WorkerUse: Send + Sync {
    fn use_secrets(&self, caller: &Caller, args: &Value) -> Option<Result<Value, ApiError>>;
}

pub struct VaultOps {
    worker: OnceLock<Arc<dyn WorkerUse>>,
    switch: Arc<SwitchFlag>,
    store: Arc<VaultStore>,
    room: Arc<dyn Room>,
    events: Option<Events>,
    approvals: Arc<Waiters<Answered<Decision>>>,
    consent: OnceLock<Arc<ConsentOps>>,
    timing: VaultTiming,
    processes: Arc<dyn Processes>,
    inbox: Option<Arc<InboxOps>>,
    sessions: Option<TrustedSession>,
}

pub type TrustedSession = Arc<dyn Fn(u32) -> Option<String> + Send + Sync>;

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

fn said_no(message: &str) -> ApiError {
    ApiError::new(ErrorCode::NeedsApproval, message)
}

impl VaultOps {
    pub fn new(switch: Arc<SwitchFlag>, store: VaultStore, room: Arc<dyn Room>) -> VaultOps {
        let ops = VaultOps {
            worker: OnceLock::new(),
            switch,
            store: Arc::new(store),
            room,
            events: None,
            approvals: Arc::new(Waiters::new("vault-approval-", MAX_APPROVALS)),
            consent: OnceLock::new(),
            timing: VaultTiming::default(),
            processes: Arc::new(SystemProcesses),
            inbox: None,
            sessions: None,
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
        .with_sessions(crate::caller_session::trusted_in(
            layout.roots(),
            Arc::new(SystemProcesses),
        ))
    }

    pub fn with_sessions(mut self, sessions: TrustedSession) -> VaultOps {
        self.sessions = Some(sessions);
        self
    }

    fn trusted_session(&self, caller: &Caller) -> Option<String> {
        let pid = caller.pid?;
        self.sessions.as_ref().and_then(|find| find(pid))
    }

    pub fn set_consent(self: &Arc<Self>, consent: &Arc<ConsentOps>) {
        if self.consent.set(consent.clone()).is_err() {
            return;
        }
        consent.register(
            SECRET_REQUEST,
            Arc::new(asks::SecretHandler {
                ops: Arc::downgrade(self),
            }),
        );
        let (room, weak) = (self.room.clone(), Arc::downgrade(consent));
        self.switch.subscribe(move |on| {
            if !on {
                return;
            }
            let Some(consent) = weak.upgrade() else {
                return;
            };
            for row in consent.store().open_rows().unwrap_or_default() {
                if row.kind == SECRET_REQUEST && row.state.open() {
                    room.publish(
                        topics::VAULT,
                        topics::VAULT_REQUEST_PENDING,
                        &asks::request_event(&row),
                    );
                }
            }
        });
    }

    pub fn set_worker(&self, worker: Arc<dyn WorkerUse>) {
        let _ = self.worker.set(worker);
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
            ops::VAULT_FIND => Some(OpSpec::AGENT_READ),
            ops::VAULT_USE | ops::VAULT_REQUEST => Some(OpSpec::AGENT_ACTS),
            op if VAULT_OPS.contains(&op) => Some(OpSpec::APP),
            _ => None,
        }
    }

    pub fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if op == ops::VAULT_USE {
            if let Some(answer) = self.worker.get().and_then(|w| w.use_secrets(caller, args)) {
                return answer;
            }
        }
        match op {
            ops::VAULT_REQUEST => return self.request(caller, args),
            ops::VAULT_REQUEST_CANCEL => return self.cancel(caller, args),
            _ => {}
        }
        if !self.switch.is_on() {
            self.off_hook().run();
            return Err(self.switch.refusal());
        }
        match op {
            ops::VAULT_FIND => self.find(args),
            ops::VAULT_USE => self.use_secrets(caller, args),
            ops::VAULT_LIST => self.entries().map(|e| json!({"entries": e})),
            ops::VAULT_SAVE => self.save(caller, args),
            ops::VAULT_REMOVE => self.remove(args),
            ops::VAULT_APPROVAL_ANSWER => self.answer(caller, args),
            ops::VAULT_PENDING => Ok(self.pending()),
            ops::VAULT_ADMIN => self.admin_secret(caller, args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    pub fn close(&self) {
        self.approvals.drain();
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
        let always = || Ok(());
        let pairs = self.release_picked(
            &Who::Caller(caller),
            &names,
            (&folder, &command),
            (trail, &always),
        )?;
        let (env, hide) = list::environment(&pairs);
        Ok(json!({"env": env, "hide": hide}))
    }

    fn admin_secret(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App || caller.client != ClientKind::App {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app may read the vault for a machine's admin password",
            ));
        }
        let bad = || ApiError::bad_request("vault.admin takes {name, machine}");
        let name = text(args, "name", MAX_SHORT)?.ok_or_else(bad)?;
        let machine = text(args, "machine", MAX_SHORT)?.ok_or_else(bad)?;
        let id = name.to_lowercase();
        if !self.entries()?.iter().any(|e| e.id() == id) {
            return Err(failed(format!("{name} isn't in the Vault.")));
        }
        let reason = format!("sudo on {machine}");
        let mut trail = Trail::new(String::new(), reason.clone());
        let always = || Ok(());
        let released = self.release_picked(
            &Who::Machine(&machine),
            std::slice::from_ref(&name),
            ("", &reason),
            (&mut trail, &always),
        );
        match released {
            Ok(pairs) => {
                let secret = pairs
                    .into_iter()
                    .next()
                    .map(|(_, value)| value.secret)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| failed(format!("{name} has no password saved.")));
                let result = if secret.is_ok() {
                    SecretResult::Given
                } else {
                    SecretResult::Failed
                };
                if !self.note_used(&trail, result, secret.is_ok()) {
                    return Err(failed(LOG_DOWN));
                }
                Ok(json!({"secret": secret?}))
            }
            Err((e, result)) => {
                if !trail.names.is_empty() {
                    self.note_used(&trail, result, false);
                }
                Err(e)
            }
        }
    }

    fn save(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let request = match text(args, "consentId", MAX_SHORT)? {
            Some(id) => Some(id),
            None => text(args, "request", MAX_SHORT)?,
        };
        if request.is_some() {
            Self::the_app(caller)?;
        }
        let entry = entry_from(args.get("entry"))?;
        let value = value_from(args.get("value"))?;
        let used = match (&request, self.consent.get()) {
            (Some(id), Some(consent)) => {
                Some((consent, consent.use_for_app(caller, id, SECRET_REQUEST, &entry.name)?))
            }
            _ => None,
        };
        let ended = |outcome: Result<Value, String>| {
            if let Some((consent, row)) = &used {
                consent.finish_use(row, outcome);
            }
        };
        let old = match text(args, "replacing", MAX_SHORT)? {
            Some(name) => match self.store.entry(&name) {
                Ok(Some(old)) => Some(old),
                Ok(None) => {
                    let why = format!("There's no secret named {name}.");
                    ended(Err(why.clone()));
                    return Err(failed(why));
                }
                Err(e) => {
                    ended(Err(e.to_string()));
                    return Err(failed(e.to_string()));
                }
            },
            None => None,
        };
        let saved = match self.store.save(&entry, value, old.as_ref(), &now_iso()) {
            Ok(saved) => saved,
            Err(e) => {
                ended(Err(e.to_string()));
                return Err(failed(e.to_string()));
            }
        };
        let mut names = vec![saved.entry.name.as_str()];
        if let Some(old) = old.as_ref().filter(|o| o.id() != saved.entry.id()) {
            names.push(old.name.as_str());
        }
        self.publish_changed(&names, saved.value_changed);
        self.note_saved(old.as_ref(), &saved);
        let answered = used.is_some();
        ended(Ok(json!({"entry": entry_json(&saved.entry)})));
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
#[path = "vault_release.rs"]
mod release;
#[path = "vault_events.rs"]
mod vault_events;

#[cfg(test)]
#[path = "vault_ops_admin_tests.rs"]
mod admin_tests;
#[cfg(test)]
#[path = "vault_ops_events_tests.rs"]
mod events_tests;
#[cfg(test)]
#[path = "vault_ops_pending_tests.rs"]
mod pending_tests;
#[cfg(test)]
#[path = "vault_release_tests.rs"]
mod release_tests;
#[cfg(test)]
#[path = "vault_ops_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "vault_ops_inbox_tests.rs"]
mod vault_ops_inbox_tests;
#[cfg(test)]
#[path = "vault_ops_worker_tests.rs"]
mod worker_tests;

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
