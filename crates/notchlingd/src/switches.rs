//! Milestone 6's switches: `accounts`, `vault` and `install`, each off until the maintainer hands
//! that area over from the Swift app (decision 0011), with M3's `acting` rule:
//!
//! - every switch is off at every daemon start, whatever any file says (a file at rest is not a
//!   tier boundary: an agent that can run a shell can write it, then restart the daemon);
//! - only an app-tier `settings.set_switch {switch, on}` during this run turns one on;
//! - the app keeps the user's choice and asserts it again after each hello.
//!
//! An area takes its [`SwitchFlag`] (an `Arc`) and asks [`SwitchFlag::is_on`] before it touches
//! anything the Swift app owns, answering [`SwitchFlag::refusal`] while off. One that holds
//! something open (the vault's socket, a sign-in listener) calls [`SwitchFlag::subscribe`] to close
//! it when the switch goes off.
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `settings.set_switch` | app | `{switch: "accounts" \| "vault" \| "install", on: bool}` | `{switch, on}` |
//! | `settings.switches` | agent | `{}` | `{accounts: bool, vault: bool, install: bool}` |
//!
//! A change is published on `settings` as `settings.switch {switch, on}`. App-tier calls are
//! written to the audit log by the server, with their outcome.

use crate::acting_ops::{bad, forbidden, Room};
use crate::op_group::OpGroup;
use notchling_local_api::{ops, topics, ApiError, AuditEntry, AuditLog, Caller, OpSpec};
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const ACCOUNTS: &str = "accounts";
pub const VAULT: &str = "vault";
pub const INSTALL: &str = "install";

/// What every account op answers while `accounts` is off (code `forbidden`).
pub const ACCOUNTS_OFF: &str =
    "accounts are off: the Swift app owns logins until the maintainer turns this on";
/// What every vault op answers while `vault` is off.
pub const VAULT_OFF: &str =
    "the vault is off: the Swift app owns it until the maintainer turns this on";
/// What install ops say beside their dry run while `install` is off.
pub const INSTALL_OFF: &str =
    "install is off: the Swift app owns hooks and skills until the maintainer turns this on";

type Listener = Arc<dyn Fn(bool) + Send + Sync>;

/// One switch, for this daemon run only.
pub struct SwitchFlag {
    name: &'static str,
    off_message: &'static str,
    on: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
    /// One change at a time, so listeners see the changes in the order they were made.
    changing: Mutex<()>,
}

impl SwitchFlag {
    /// Off: how every daemon run starts.
    pub fn off(name: &'static str, off_message: &'static str) -> Arc<SwitchFlag> {
        SwitchFlag::in_memory(name, off_message, false)
    }

    /// A switch set as given (tests).
    pub fn in_memory(name: &'static str, off_message: &'static str, on: bool) -> Arc<SwitchFlag> {
        Arc::new(SwitchFlag {
            name,
            off_message,
            on: AtomicBool::new(on),
            listeners: Mutex::new(Vec::new()),
            changing: Mutex::new(()),
        })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    /// `forbidden` with the switch's off message.
    pub fn refusal(&self) -> ApiError {
        forbidden(self.off_message)
    }

    /// `Ok` while on, else [`SwitchFlag::refusal`].
    pub fn check(&self) -> Result<(), ApiError> {
        if self.is_on() {
            Ok(())
        } else {
            Err(self.refusal())
        }
    }

    /// Calls `listener` with the new state after each change, on the thread that made it, after
    /// the flag has flipped. A listener must not set this switch itself.
    pub fn subscribe(&self, listener: impl Fn(bool) + Send + Sync + 'static) {
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::new(listener));
    }

    /// Flips it; true when that changed it, and only then are the listeners called. Only
    /// `settings.set_switch` calls this in the daemon.
    pub(crate) fn set(&self, on: bool) -> bool {
        let _one = self.changing.lock().unwrap_or_else(|e| e.into_inner());
        if self.on.swap(on, Ordering::SeqCst) == on {
            return false;
        }
        let listeners: Vec<Listener> = self
            .listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for listener in listeners {
            listener(on);
        }
        true
    }
}

/// The three switches.
pub struct Switches {
    pub accounts: Arc<SwitchFlag>,
    pub vault: Arc<SwitchFlag>,
    pub install: Arc<SwitchFlag>,
}

impl Switches {
    /// All off: how every daemon run starts.
    pub fn off() -> Switches {
        Switches {
            accounts: SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF),
            vault: SwitchFlag::off(VAULT, VAULT_OFF),
            install: SwitchFlag::off(INSTALL, INSTALL_OFF),
        }
    }

    pub fn all(&self) -> [&Arc<SwitchFlag>; 3] {
        [&self.accounts, &self.vault, &self.install]
    }

    pub fn get(&self, name: &str) -> Option<&Arc<SwitchFlag>> {
        self.all().into_iter().find(|s| s.name() == name)
    }

    /// `{accounts: bool, vault: bool, install: bool}`.
    pub fn to_json(&self) -> Value {
        let map: Map<String, Value> = self
            .all()
            .iter()
            .map(|s| (s.name().to_string(), json!(s.is_on())))
            .collect();
        Value::Object(map)
    }
}

/// The two ops.
pub struct SwitchOps {
    switches: Arc<Switches>,
    room: Arc<dyn Room>,
    /// Each change, with the switch's name (the server's own line names only the op).
    audit: Option<AuditLog>,
}

impl SwitchOps {
    pub fn new(switches: Arc<Switches>, room: Arc<dyn Room>) -> SwitchOps {
        SwitchOps {
            switches,
            room,
            audit: None,
        }
    }

    /// Writes `settings.set_switch <name>` with `on` or `off` to `audit` on each change.
    pub fn with_audit(mut self, audit: AuditLog) -> SwitchOps {
        self.audit = Some(audit);
        self
    }

    fn set(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let usage = "settings.set_switch takes {\"switch\": \"accounts\"|\"vault\"|\"install\", \"on\": true|false}";
        let name = args
            .get("switch")
            .and_then(Value::as_str)
            .ok_or_else(|| bad(usage))?;
        let on = args
            .get("on")
            .and_then(Value::as_bool)
            .ok_or_else(|| bad(usage))?;
        let flag = self.switches.get(name).ok_or_else(|| bad(usage))?;
        flag.set(on);
        if let Some(audit) = &self.audit {
            let op = format!("{} {}", ops::SETTINGS_SET_SWITCH, flag.name());
            let result = if on { "on" } else { "off" };
            let entry = AuditEntry::now(&op, Some(caller.tier), caller.peer.as_deref(), result);
            let _ = audit.append(&entry);
        }
        let data = json!({"switch": flag.name(), "on": on});
        self.room
            .publish(topics::SETTINGS, topics::SWITCH_CHANGED, &data);
        Ok(data)
    }
}

impl OpGroup for SwitchOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SETTINGS_SET_SWITCH => Some(OpSpec::APP),
            ops::SETTINGS_SWITCHES => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SETTINGS_SET_SWITCH => self.set(caller, args),
            _ => Ok(self.switches.to_json()),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::SETTINGS]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acting_ops_tests::{app, FakeRoom};
    use notchling_local_api::ErrorCode;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn every_switch_starts_off() {
        let s = Switches::off();
        assert_eq!(
            s.to_json(),
            json!({"accounts": false, "vault": false, "install": false})
        );
        assert!(s.all().iter().all(|f| !f.is_on()));
        assert_eq!(s.accounts.check().unwrap_err().message, ACCOUNTS_OFF);
        assert_eq!(s.vault.refusal().code, ErrorCode::Forbidden);
    }

    #[test]
    fn listeners_hear_each_change_once() {
        let flag = SwitchFlag::off(VAULT, VAULT_OFF);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        flag.subscribe(move |on| h.lock().unwrap().push(on));
        assert!(flag.set(true));
        assert!(!flag.set(true), "no change, no call");
        assert!(flag.set(false));
        assert_eq!(*heard.lock().unwrap(), vec![true, false]);
        assert!(flag.check().is_err());
    }

    #[test]
    fn the_op_sets_one_switch_and_says_so() {
        let room = Arc::new(FakeRoom::default());
        let switches = Arc::new(Switches::off());
        let log =
            std::env::temp_dir().join(format!("nl-switch-audit-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let ops =
            SwitchOps::new(switches.clone(), room.clone()).with_audit(AuditLog::new(log.clone()));
        assert_eq!(ops.spec(ops::SETTINGS_SET_SWITCH), Some(OpSpec::APP));
        assert_eq!(ops.spec(ops::SETTINGS_SWITCHES), Some(OpSpec::AGENT));
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        switches.accounts.subscribe(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        let got = ops
            .call(
                &app(),
                ops::SETTINGS_SET_SWITCH,
                &json!({"switch": "accounts", "on": true}),
            )
            .unwrap();
        assert_eq!(got, json!({"switch": "accounts", "on": true}));
        assert!(switches.accounts.is_on() && !switches.vault.is_on());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(room.named("settings.switch"), vec![got]);
        let read = ops
            .call(&app(), ops::SETTINGS_SWITCHES, &json!({}))
            .unwrap();
        assert_eq!(
            read,
            json!({"accounts": true, "vault": false, "install": false})
        );
        for bad_args in [
            json!({"switch": "acting", "on": true}),
            json!({"switch": "vault"}),
            json!({"on": true}),
        ] {
            let e = ops
                .call(&app(), ops::SETTINGS_SET_SWITCH, &bad_args)
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::BadRequest, "{bad_args}");
        }
        assert!(!switches.vault.is_on());
        let written = std::fs::read_to_string(&log).unwrap();
        assert_eq!(written.lines().count(), 1, "{written}");
        assert!(
            written.contains("\"op\":\"settings.set_switch accounts\""),
            "{written}"
        );
        assert!(written.contains("\"result\":\"on\""), "{written}");
        let _ = std::fs::remove_file(&log);
    }
}
