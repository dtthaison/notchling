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

pub const TOPICS: [&str; 1] = [topics::PROVIDERS];

const REPLY_WAIT: Duration = Duration::from_millis(4_500);
const POLL_EVERY: Duration = Duration::from_secs(2);
const ADD_LIMIT: Duration = Duration::from_secs(15 * 60);

struct Shared {
    accounts: Arc<SwitchFlag>,
    registry: ProviderRegistry,
    room: Arc<dyn Room>,
    reply_wait_ms: AtomicU64,
    poll_ms: AtomicU64,
    add_limit_ms: AtomicU64,
    closing: AtomicBool,
    busy: Mutex<HashSet<String>>,
    adding: Mutex<HashSet<String>>,
    syncing: Mutex<HashSet<String>>,
    wake: Condvar,
}

impl Shared {
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

struct SyncClaim {
    shared: Arc<Shared>,
    key: String,
}

impl SyncClaim {
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
        let weak = Arc::downgrade(&sh);
        accounts.subscribe(move |on| {
            if let (false, Some(sh)) = (on, weak.upgrade()) {
                sh.wake_all();
            }
        });
        ProvidersOps { sh }
    }

    pub fn set_reply_wait(&self, wait: Duration) {
        self.sh
            .reply_wait_ms
            .store(wait.as_millis() as u64, Ordering::SeqCst);
    }

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
