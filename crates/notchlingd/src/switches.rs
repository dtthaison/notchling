use crate::acting_ops::{bad, forbidden, Room};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use notchling_events::kinds::choices::SwitchName;
use notchling_events::kinds::security::{SwitchCause, SwitchChanged};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event};
use notchling_local_api::{ops, topics, ApiError, Caller, OpSpec};
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ACCOUNTS: &str = "accounts";
pub const VAULT: &str = "vault";
pub const INSTALL: &str = "install";
pub const CLUSTER: &str = "cluster";

pub const ACCOUNTS_OFF: &str =
    "accounts are off: the Swift app owns logins until the maintainer turns this on";
pub const VAULT_OFF: &str =
    "the vault is off: the Swift app owns it until the maintainer turns this on";
pub const INSTALL_OFF: &str =
    "install is off: the Swift app owns hooks and skills until the maintainer turns this on";
pub const CLUSTER_OFF: &str =
    "the cluster is off: this Mac links to no other machine until the maintainer turns this on";

type Listener = Arc<dyn Fn(bool) + Send + Sync>;
type Listeners = Arc<Mutex<Vec<Listener>>>;
type Change = (bool, SyncSender<()>);

const LISTENER_WAIT: Duration = Duration::from_millis(500);

pub struct SwitchFlag {
    name: &'static str,
    off_message: &'static str,
    on: AtomicBool,
    listeners: Listeners,
    changing: Mutex<Option<Sender<Change>>>,
}

fn listen(name: &str, listeners: Listeners) -> Option<Sender<Change>> {
    let (tx, rx) = channel::<Change>();
    std::thread::Builder::new()
        .name(format!("switch-{name}"))
        .spawn(move || {
            for (on, heard) in rx {
                let now: Vec<Listener> =
                    listeners.lock().unwrap_or_else(|e| e.into_inner()).clone();
                for listener in now {
                    listener(on);
                }
                let _ = heard.try_send(());
            }
        })
        .ok()?;
    Some(tx)
}

impl SwitchFlag {
    pub fn off(name: &'static str, off_message: &'static str) -> Arc<SwitchFlag> {
        SwitchFlag::in_memory(name, off_message, false)
    }

    pub fn in_memory(name: &'static str, off_message: &'static str, on: bool) -> Arc<SwitchFlag> {
        Arc::new(SwitchFlag {
            name,
            off_message,
            on: AtomicBool::new(on),
            listeners: Arc::new(Mutex::new(Vec::new())),
            changing: Mutex::new(None),
        })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    pub fn refusal(&self) -> ApiError {
        forbidden(self.off_message)
    }

    pub fn check(&self) -> Result<(), ApiError> {
        if self.is_on() {
            Ok(())
        } else {
            Err(self.refusal())
        }
    }

    pub fn subscribe(&self, listener: impl Fn(bool) + Send + Sync + 'static) {
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::new(listener));
    }

    pub(crate) fn set(&self, on: bool) -> bool {
        let heard = {
            let mut queue = self.changing.lock().unwrap_or_else(|e| e.into_inner());
            if self.on.swap(on, Ordering::SeqCst) == on {
                return false;
            }
            if queue.is_none() {
                *queue = listen(self.name, self.listeners.clone());
            }
            let (tx, rx) = sync_channel(1);
            match queue.as_ref().map(|q| q.send((on, tx))) {
                Some(Ok(())) => rx,
                _ => {
                    *queue = None;
                    let now: Vec<Listener> = self
                        .listeners
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    for listener in now {
                        listener(on);
                    }
                    return true;
                }
            }
        };
        let _ = heard.recv_timeout(LISTENER_WAIT);
        true
    }
}

pub struct Switches {
    pub accounts: Arc<SwitchFlag>,
    pub vault: Arc<SwitchFlag>,
    pub install: Arc<SwitchFlag>,
    pub cluster: Arc<SwitchFlag>,
}

impl Switches {
    pub fn off() -> Switches {
        Switches {
            accounts: SwitchFlag::off(ACCOUNTS, ACCOUNTS_OFF),
            vault: SwitchFlag::off(VAULT, VAULT_OFF),
            install: SwitchFlag::off(INSTALL, INSTALL_OFF),
            cluster: SwitchFlag::off(CLUSTER, CLUSTER_OFF),
        }
    }

    pub fn all(&self) -> [&Arc<SwitchFlag>; 4] {
        [&self.accounts, &self.vault, &self.install, &self.cluster]
    }

    pub fn get(&self, name: &str) -> Option<&Arc<SwitchFlag>> {
        self.all().into_iter().find(|s| s.name() == name)
    }

    pub fn to_json(&self) -> Value {
        let map: Map<String, Value> = self
            .all()
            .iter()
            .map(|s| (s.name().to_string(), json!(s.is_on())))
            .collect();
        Value::Object(map)
    }
}

pub struct SwitchOps {
    switches: Arc<Switches>,
    room: Arc<dyn Room>,
    events: Option<Events>,
}

impl SwitchOps {
    pub fn new(switches: Arc<Switches>, room: Arc<dyn Room>) -> SwitchOps {
        SwitchOps {
            switches,
            room,
            events: None,
        }
    }

    pub fn with_events(mut self, events: Events) -> SwitchOps {
        self.events = Some(events);
        self
    }

    fn set(&self, _caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let usage = "settings.set_switch takes {\"switch\": \"accounts\"|\"vault\"|\"install\"|\"cluster\", \"on\": true|false}";
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
        if let Some(events) = &self.events {
            let cause = match args.get("restored").and_then(Value::as_bool) {
                Some(true) => SwitchCause::RestoredAtLaunch,
                _ => SwitchCause::User,
            };
            let switch = SwitchName::from_name(flag.name()).unwrap_or(SwitchName::Other);
            events.push(Event::new(
                &SwitchChanged { switch, on, cause },
                Outcome::Ok,
                crate::acting_ops::now_ms(),
            ));
            scope::mark_recorded();
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
    use std::time::{Duration, Instant};

    #[test]
    fn every_switch_starts_off() {
        let s = Switches::off();
        assert_eq!(
            s.to_json(),
            json!({"accounts": false, "vault": false, "install": false, "cluster": false})
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
    fn a_slow_listener_holds_up_no_change_and_hears_them_in_order() {
        let flag = SwitchFlag::in_memory(VAULT, VAULT_OFF, true);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let h = heard.clone();
        flag.subscribe(move |on| {
            let first = {
                let mut heard = h.lock().unwrap();
                heard.push(on);
                heard.len() == 1
            };
            if first {
                let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(30));
            }
        });
        let started = Instant::now();
        let (done_tx, done) = std::sync::mpsc::channel();
        {
            let flag = flag.clone();
            std::thread::spawn(move || {
                flag.set(false);
                flag.set(true);
                flag.set(false);
                let _ = done_tx.send(());
            });
        }
        let finished = done.recv_timeout(Duration::from_secs(5));
        release.send(()).unwrap();
        assert!(finished.is_ok(), "set waited for a stuck listener");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!flag.is_on(), "off, on, off ends off");
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard.lock().unwrap().len() < 3 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(*heard.lock().unwrap(), vec![false, true, false]);
    }

    #[test]
    fn the_op_sets_one_switch_and_says_so() {
        let room = Arc::new(FakeRoom::default());
        let switches = Arc::new(Switches::off());
        let sink = Arc::new(notchling_events::MemorySink::new());
        let ops = SwitchOps::new(switches.clone(), room.clone())
            .with_events(Events::fixed(sink.clone(), notchling_events::Salt([1; 32])));
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
            json!({"accounts": true, "vault": false, "install": false, "cluster": false})
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
        let changed = crate::event_assert::of(&sink, "switch.changed");
        assert_eq!(changed.len(), 1);
        let text = |name: &str| crate::event_assert::text(&changed[0], name);
        assert_eq!(text("switch").as_deref(), Some("accounts"));
        assert_eq!(text("on").as_deref(), Some("true"));
        assert_eq!(text("cause").as_deref(), Some("user"));
    }

    #[test]
    fn a_restored_switch_says_restored_at_launch() {
        let sink = Arc::new(notchling_events::MemorySink::new());
        let switches = Arc::new(Switches::off());
        let ops = SwitchOps::new(switches, Arc::new(FakeRoom::default()))
            .with_events(Events::fixed(sink.clone(), notchling_events::Salt([1; 32])));
        ops.call(
            &app(),
            ops::SETTINGS_SET_SWITCH,
            &json!({"switch": "vault", "on": true, "restored": true}),
        )
        .unwrap();
        let changed = crate::event_assert::of(&sink, "switch.changed");
        assert_eq!(
            crate::event_assert::text(&changed[0], "cause").as_deref(),
            Some("restored_at_launch")
        );
    }
}
