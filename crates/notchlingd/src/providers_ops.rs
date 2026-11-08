//! Milestone 6's provider ops (E8b): the accounts of Codex, Antigravity and Claude Desktop over
//! [`notchling_engine::providers`]. App tier (audit-logged by the server like every app-tier op) and
//! behind the `accounts` switch, which is off at every start.
//!
//! While the switch is off every op answers `forbidden` with [`ACCOUNTS_OFF`], and nothing runs: no
//! Keychain, no process, no network, no file write, no read of a login. The one exception is
//! `providers.list`, which lists the saved account names from the daemon's own files (the plan:
//! "listing saved account names from the daemon's own files still works"); it says `accounts_on:
//! false`, names no active account, and does not even ask whether an app is installed.
//!
//! | Op | Args | Reply |
//! | --- | --- | --- |
//! | `providers.list` | `{sync?: bool}` (default true) | `{"accounts_on", "providers": [{id, title, capabilities, available, state: {accounts: [{id, email, plan, needs_login, switchable}], active_id, busy, adding, sign_in_url, unlisted_active_title}}], "syncing"?: true}` |
//! | `providers.usage` | `{provider, refresh?: bool}` | `{"provider", "reports": {<account id>: {fetched, groups: [{name, bars: [{label, kind, percent, resets_at}]}]}}, "refreshing"?: true}` |
//! | `providers.switch` | `{provider, id, confirm?: bool}` | `{"state": "switched", provider, id}`, `{"state": "needs_confirmation", confirmation: {title, message, button}}` (pass `confirm: true` after asking), or `{"state": "switching"}` when it takes over 4.5 s; code `failed` with the reason otherwise |
//! | `providers.add.begin` | `{provider, confirm?: bool}` | `{"state": "waiting", provider, sign_in_url?}`, `needs_confirmation` as above, or `{"state": "starting"}` |
//! | `providers.add.cancel` | `{provider}` | `{"cancelled": true}`, or `{"state": "cancelling"}` |
//! | `providers.remove` | `{provider, id}` | `{"removed": true}`: forgets the saved login; the account stays signed in if it is the one in use |
//!
//! `provider` is `codex`, `antigravity` or `claudeDesktop`. Events on topic `providers` (app tier):
//! `providers.changed {provider, reason: "sync" | "usage" | "switch" | "add" | "remove", state?,
//! id?, error?, sign_in_url?}`. An add flow's progress (`started`, `waiting` with the sign-in page
//! once Codex prints it, then `done`, `cancelled` or `failed`) arrives as `reason: "add"` events; a
//! switch or refresh that outlasts its reply ends as one too. No token, cookie or login is in a
//! reply or an event; a failure's text is fixed words.
//!
//! One switch or add flow per provider at a time. A flow that waits for the user stops being
//! polled the moment the switch turns off and is abandoned (`AccountProvider::abandon_add`: Codex's
//! `codex login` stopped and its scratch home removed; nothing written or restored, the event's
//! `note` says where a provider was left), and is cancelled when the daemon stops while it is on.

use crate::acting_ops::{bad, text_arg, Room};
use crate::op_group::OpGroup;
use notchling_engine::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Confirmation, ProviderRegistry,
    ProviderState,
};
use notchling_local_api::{ops, topics, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub use crate::switches::{SwitchFlag, ACCOUNTS_OFF};

pub const PROVIDERS_OPS: [&str; 6] = [
    ops::PROVIDERS_LIST,
    ops::PROVIDERS_SWITCH,
    ops::PROVIDERS_ADD_BEGIN,
    ops::PROVIDERS_ADD_CANCEL,
    ops::PROVIDERS_REMOVE,
    ops::PROVIDERS_USAGE,
];

/// The topics these ops publish on.
pub const TOPICS: [&str; 1] = [topics::PROVIDERS];

/// How long a reply waits for a switch, a refresh or an add to start (the call is given up at 5 s).
const REPLY_WAIT: Duration = Duration::from_millis(4_500);
/// How often an add flow looks for the user's sign-in.
const POLL_EVERY: Duration = Duration::from_secs(2);
/// How long an add flow may wait for the user.
const ADD_LIMIT: Duration = Duration::from_secs(15 * 60);

struct Shared {
    accounts: Arc<SwitchFlag>,
    registry: ProviderRegistry,
    room: Arc<dyn Room>,
    reply_wait_ms: AtomicU64,
    poll_ms: AtomicU64,
    add_limit_ms: AtomicU64,
    closing: AtomicBool,
    /// Providers with a switch or an add flow under way.
    busy: Mutex<HashSet<String>>,
    /// Providers whose add flow is waiting for the user.
    adding: Mutex<HashSet<String>>,
    /// Providers with a sync (a list's or a usage refresh's) on a thread now: one each at most, so
    /// a sync stuck on a Keychain prompt doesn't gather a thread per call behind it.
    syncing: Mutex<HashSet<String>>,
    /// Wakes a waiting add flow early: the switch went off or the daemon is stopping.
    wake: Condvar,
}

impl Shared {
    /// Waits up to `wait`, or until the switch is off or the daemon stopping. Checked under the
    /// lock [`Shared::wake_all`] notifies under, so a change made before the wait began still
    /// ends it.
    fn nap(&self, wait: Duration) {
        let adding = lock(&self.adding);
        let _ = self.wake.wait_timeout_while(adding, wait, |_| {
            self.accounts.is_on() && !self.closing.load(Ordering::SeqCst)
        });
    }

    fn wake_all(&self) {
        let _adding = lock(&self.adding);
        self.wake.notify_all();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A provider's sync claimed until dropped (also when a thread that would have run it never
/// started, since the closure holding it is dropped then).
struct SyncClaim {
    shared: Arc<Shared>,
    key: String,
}

impl SyncClaim {
    /// `None` when that provider's sync is already going.
    fn take(shared: &Arc<Shared>, provider: &str) -> Option<SyncClaim> {
        lock(&shared.syncing)
            .insert(provider.to_string())
            .then(|| SyncClaim {
                shared: shared.clone(),
                key: provider.to_string(),
            })
    }
}

impl Drop for SyncClaim {
    fn drop(&mut self) {
        lock(&self.shared.syncing).remove(&self.key);
    }
}

/// Holds a provider's busy slot until dropped.
struct Slot {
    shared: Arc<Shared>,
    key: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        lock(&self.shared.busy).remove(&self.key);
        lock(&self.shared.adding).remove(&self.key);
    }
}

pub struct ProvidersOps {
    sh: Arc<Shared>,
}

impl ProvidersOps {
    pub fn new(
        accounts: Arc<SwitchFlag>,
        registry: ProviderRegistry,
        room: Arc<dyn Room>,
    ) -> ProvidersOps {
        let sh = Arc::new(Shared {
            accounts: accounts.clone(),
            registry,
            room,
            reply_wait_ms: AtomicU64::new(REPLY_WAIT.as_millis() as u64),
            poll_ms: AtomicU64::new(POLL_EVERY.as_millis() as u64),
            add_limit_ms: AtomicU64::new(ADD_LIMIT.as_millis() as u64),
            closing: AtomicBool::new(false),
            busy: Mutex::new(HashSet::new()),
            adding: Mutex::new(HashSet::new()),
            syncing: Mutex::new(HashSet::new()),
            wake: Condvar::new(),
        });
        // Off: a flow waiting for the user stops at once and touches nothing more (it doesn't
        // cancel the provider's own flow either, which could write a login back).
        let weak = Arc::downgrade(&sh);
        accounts.subscribe(move |on| {
            if let (false, Some(sh)) = (on, weak.upgrade()) {
                sh.wake_all();
            }
        });
        ProvidersOps { sh }
    }

    /// How long a reply waits before it says the job is still going (tests shorten it).
    pub fn set_reply_wait(&self, wait: Duration) {
        self.sh
            .reply_wait_ms
            .store(wait.as_millis() as u64, Ordering::SeqCst);
    }

    /// How often an add flow polls, and how long it may wait (tests shorten both).
    pub fn set_add_timing(&self, poll: Duration, limit: Duration) {
        self.sh
            .poll_ms
            .store(poll.as_millis() as u64, Ordering::SeqCst);
        self.sh
            .add_limit_ms
            .store(limit.as_millis() as u64, Ordering::SeqCst);
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        PROVIDERS_OPS.contains(&op).then_some(OpSpec::APP)
    }

    /// The daemon is stopping: an add flow still waiting for the user is cancelled (while the
    /// switch is on), and the polling threads end.
    pub fn close(&self) {
        self.sh.closing.store(true, Ordering::SeqCst);
        self.sh.wake_all();
        if !self.sh.accounts.is_on() {
            return;
        }
        let waiting: Vec<String> = lock(&self.sh.adding).iter().cloned().collect();
        for id in waiting {
            if let Some(p) = self.sh.registry.get(&id) {
                p.cancel_add();
            }
        }
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        if !self.sh.accounts.is_on() {
            return match op {
                ops::PROVIDERS_LIST => Ok(self.list_off()),
                _ => Err(self.sh.accounts.refusal()),
            };
        }
        match op {
            ops::PROVIDERS_LIST => Ok(self.list_on(args)),
            ops::PROVIDERS_USAGE => self.usage(args),
            ops::PROVIDERS_SWITCH => self.switch(args),
            ops::PROVIDERS_ADD_BEGIN => self.add_begin(args),
            ops::PROVIDERS_ADD_CANCEL => self.add_cancel(args),
            ops::PROVIDERS_REMOVE => self.remove(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    // MARK: Helpers

    fn wait(&self) -> Duration {
        Duration::from_millis(self.sh.reply_wait_ms.load(Ordering::SeqCst))
    }

    fn provider(&self, args: &Value) -> Result<Arc<dyn AccountProvider>, ApiError> {
        let id = text_arg(args, "provider")?;
        self.sh
            .registry
            .get(id)
            .ok_or_else(|| bad(format!("no provider named {id}")))
    }

    fn account_id(args: &Value) -> Result<String, ApiError> {
        let id = text_arg(args, "id")?;
        if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
            return Err(bad("id is not an account id"));
        }
        Ok(id.to_string())
    }

    fn flag(args: &Value, key: &str, default: bool) -> bool {
        args.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    /// Runs `work` on a thread of its own and waits for it up to the reply wait; `None` when it is
    /// still going (it publishes its own end as an event).
    fn deferred<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("providers-op".into())
            .spawn(move || {
                let _ = tx.send(work());
            });
        spawned.ok()?;
        rx.recv_timeout(self.wait()).ok()
    }

    /// Takes the provider's switch-or-add slot; code `failed` when something else holds it.
    fn take_slot(&self, provider: &str) -> Result<Slot, ApiError> {
        if !lock(&self.sh.busy).insert(provider.to_string()) {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "another account change is under way for this provider",
            ));
        }
        Ok(Slot {
            shared: self.sh.clone(),
            key: provider.to_string(),
        })
    }

    fn changed(sh: &Shared, data: Value) {
        sh.room
            .publish(topics::PROVIDERS, topics::PROVIDERS_CHANGED, &data);
    }

    // MARK: Reads

    fn state_json(state: &ProviderState, masked: bool) -> Value {
        json!({
            "accounts": state.accounts.iter().map(|a| json!({
                "id": a.id, "email": a.email, "plan": a.plan,
                "needs_login": a.needs_login, "switchable": a.switchable,
            })).collect::<Vec<_>>(),
            "active_id": if masked { None } else { state.active_id.clone() },
            "busy": !masked && state.busy,
            "adding": !masked && state.adding,
            "sign_in_url": if masked { None } else { state.sign_in_url.clone() },
            "unlisted_active_title": state.unlisted_active_title,
        })
    }

    fn provider_json(p: &dyn AccountProvider, available: Option<bool>, masked: bool) -> Value {
        json!({
            "id": p.id().as_str(),
            "title": p.title(),
            "capabilities": p.capabilities().names(),
            "available": available,
            "state": Self::state_json(&p.state(), masked),
        })
    }

    /// The saved names, with nothing read from a login, an app or a process.
    fn list_off(&self) -> Value {
        json!({
            "accounts_on": false,
            "providers": self.sh.registry.providers().iter()
                .map(|p| Self::provider_json(p.as_ref(), None, true))
                .collect::<Vec<_>>(),
        })
    }

    fn list_on(&self, args: &Value) -> Value {
        let sync = Self::flag(args, "sync", true);
        let sh = self.sh.clone();
        // Each provider's sync is claimed before the thread starts; one already syncing is left
        // to the thread that has it, and with every one taken no thread starts at all.
        let claimed: Vec<Option<SyncClaim>> = sh
            .registry
            .providers()
            .iter()
            .map(|p| {
                sync.then(|| SyncClaim::take(&sh, p.id().as_str()))
                    .flatten()
            })
            .collect();
        let reads = if sync && claimed.iter().all(Option::is_none) {
            None
        } else {
            self.deferred(move || {
                let mut available = vec![];
                for (p, mine) in sh.registry.providers().iter().zip(claimed) {
                    if !sh.accounts.is_on() {
                        break;
                    }
                    if let Some(claim) = mine {
                        p.sync(false);
                        drop(claim);
                    }
                    available.push(p.is_available());
                }
                Self::changed(&sh, json!({"provider": null, "reason": "sync"}));
                available
            })
        };
        let providers: Vec<Value> = self
            .sh
            .registry
            .providers()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let available = reads.as_ref().and_then(|a| a.get(i).copied());
                Self::provider_json(p.as_ref(), available, false)
            })
            .collect();
        let mut reply = json!({"accounts_on": true, "providers": providers});
        if reads.is_none() {
            reply["syncing"] = json!(true);
        }
        reply
    }

    fn usage(&self, args: &Value) -> Result<Value, ApiError> {
        let p = self.provider(args)?;
        let refresh = Self::flag(args, "refresh", false);
        let sh = self.sh.clone();
        let worker = p.clone();
        let done = match SyncClaim::take(&sh, p.id().as_str()) {
            Some(claim) => self.deferred(move || {
                if sh.accounts.is_on() {
                    worker.sync(refresh);
                }
                drop(claim);
                Self::changed(
                    &sh,
                    json!({"provider": worker.id().as_str(), "reason": "usage"}),
                );
            }),
            None => None,
        };
        let mut reports = serde_json::Map::new();
        for account in p.state().accounts {
            if let Some(report) = p.usage_report(&account.id) {
                reports.insert(account.id, report.to_json());
            }
        }
        let mut reply = json!({"provider": p.id().as_str(), "reports": reports});
        if done.is_none() {
            reply["refreshing"] = json!(true);
        }
        Ok(reply)
    }

    // MARK: Changes

    fn switch(&self, args: &Value) -> Result<Value, ApiError> {
        let p = self.provider(args)?;
        let id = Self::account_id(args)?;
        let confirmed = Self::flag(args, "confirm", false);
        let slot = self.take_slot(p.id().as_str())?;
        let sh = self.sh.clone();
        let (worker, target) = (p.clone(), id.clone());
        let outcome = self.deferred(move || {
            let _slot = slot;
            if !confirmed {
                if let Some(c) = worker.switch_confirmation(&target) {
                    return Outcome::Confirm(c);
                }
            }
            let provider = worker.id().as_str().to_string();
            match worker.switch_to(&target) {
                Ok(()) => {
                    Self::changed(&sh, json!({"provider": provider, "reason": "switch", "id": target, "state": "done"}));
                    Outcome::Done
                }
                Err(e) => {
                    Self::changed(&sh, json!({
                        "provider": provider, "reason": "switch", "id": target, "state": "failed",
                        "title": worker.switch_failure_title(), "error": e.message(),
                    }));
                    Outcome::Failed(e)
                }
            }
        });
        match outcome {
            Some(Outcome::Done) => {
                Ok(json!({"state": "switched", "provider": p.id().as_str(), "id": id}))
            }
            Some(Outcome::Failed(e)) => Err(ApiError::new(ErrorCode::Failed, e.message())),
            Some(Outcome::Confirm(c)) => Ok(confirmation_json(&c)),
            None => Ok(json!({"state": "switching"})),
        }
    }

    fn add_begin(&self, args: &Value) -> Result<Value, ApiError> {
        let p = self.provider(args)?;
        if !p
            .capabilities()
            .contains(notchling_engine::providers::Capabilities::ADD_ACCOUNT)
        {
            return Err(bad("this provider can't add accounts from Notchling"));
        }
        let confirmed = Self::flag(args, "confirm", false);
        let slot = self.take_slot(p.id().as_str())?;
        let (tx, rx) = channel();
        let (sh, worker) = (self.sh.clone(), p.clone());
        let spawned = std::thread::Builder::new()
            .name("providers-add".into())
            .spawn(move || add_flow(sh, worker, confirmed, slot, tx));
        if spawned.is_err() {
            return Err(ApiError::internal("couldn't start the sign-in"));
        }
        match rx.recv_timeout(self.wait()) {
            Ok(First::Confirm(c)) => Ok(confirmation_json(&c)),
            Ok(First::Started(Ok(started))) => Ok(json!({
                "state": "waiting", "provider": p.id().as_str(), "sign_in_url": started.sign_in_url,
            })),
            Ok(First::Started(Err(e))) => Err(ApiError::new(ErrorCode::Failed, e.message())),
            Err(_) => Ok(json!({"state": "starting"})),
        }
    }

    fn add_cancel(&self, args: &Value) -> Result<Value, ApiError> {
        let p = self.provider(args)?;
        let done = self.deferred(move || p.cancel_add());
        Ok(if done.is_some() {
            json!({"cancelled": true})
        } else {
            json!({"state": "cancelling"})
        })
    }

    fn remove(&self, args: &Value) -> Result<Value, ApiError> {
        let p = self.provider(args)?;
        let id = Self::account_id(args)?;
        let sh = self.sh.clone();
        let done = self.deferred(move || {
            p.remove(&id);
            Self::changed(
                &sh,
                json!({"provider": p.id().as_str(), "reason": "remove", "id": id}),
            );
        });
        Ok(json!({"removed": done.is_some()}))
    }
}

impl OpGroup for ProvidersOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        ProvidersOps::spec(self, op)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        ProvidersOps::call(self, op, args)
    }

    fn topics(&self) -> &[&'static str] {
        &TOPICS
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        TOPICS.contains(&topic).then_some(Tier::App)
    }
}

enum Outcome {
    Done,
    Failed(AccountError),
    Confirm(Confirmation),
}

enum First {
    Confirm(Confirmation),
    Started(Result<AddStarted, AccountError>),
}

fn confirmation_json(c: &Confirmation) -> Value {
    json!({
        "state": "needs_confirmation",
        "confirmation": {"title": c.title, "message": c.message, "button": c.button},
    })
}

#[path = "providers_add.rs"]
mod add;
use add::add_flow;
