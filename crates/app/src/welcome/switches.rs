//! The daemon's three Milestone 6 switches as the app sees them, and the user's choice for each.
//!
//! Like `acting` and the bridge, they are off at every daemon start whatever any file says
//! (`settings.set_switch`, notchlingd's `switches`). The choice is the app's to keep (`accountsOn`,
//! `vaultOn`, `installOn` in the app's own `settings.json`): [`Switches::refresh`] reads
//! `settings.switches` and sends `settings.set_switch {on: true}` again for each switch the user
//! chose on that the daemon says is off. Only [`Switches::choose`], called after a confirmation,
//! ever changes the choice.
//!
//! Every call blocks, so the caller makes it off the UI thread.

use std::sync::Mutex;

use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::{json, Value};

use super::own::{choice_key, Flags};
use crate::acting::{self, Acting};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;

pub const ACCOUNTS: &str = "accounts";
pub const VAULT: &str = "vault";
pub const INSTALL: &str = "install";
/// The switches, in the order they are shown.
pub const NAMES: [&str; 3] = [ACCOUNTS, VAULT, INSTALL];

pub const OP_SET: &str = "settings.set_switch";
pub const OP_GET: &str = "settings.switches";

/// What the daemon said about its switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct States {
    pub accounts: bool,
    pub vault: bool,
    pub install: bool,
}

impl States {
    pub fn get(&self, name: &str) -> bool {
        match name {
            ACCOUNTS => self.accounts,
            VAULT => self.vault,
            INSTALL => self.install,
            _ => false,
        }
    }

    fn with(mut self, name: &str, on: bool) -> States {
        match name {
            ACCOUNTS => self.accounts = on,
            VAULT => self.vault = on,
            INSTALL => self.install = on,
            _ => {}
        }
        self
    }

    /// Reads `settings.switches`. A switch the reply doesn't name is off, since off is the default.
    pub fn parse(reply: &Value) -> Option<States> {
        reply.as_object()?;
        let flag = |k: &str| reply.get(k).and_then(Value::as_bool).unwrap_or(false);
        Some(States {
            accounts: flag(ACCOUNTS),
            vault: flag(VAULT),
            install: flag(INSTALL),
        })
    }
}

/// What the app last knew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    /// No daemon answered.
    Detached,
    States(States),
}

pub struct Switches {
    transport: Option<Box<dyn Transport>>,
    known: Mutex<Known>,
    flags: Option<Flags>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A refusal's line: the daemon's own message, else the error's.
fn message(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

impl Switches {
    /// Over a transport, keeping the choice in `flags`; detached until the daemon answers.
    pub fn over(transport: Box<dyn Transport>, flags: Option<Flags>) -> Switches {
        Switches {
            transport: Some(transport),
            known: Mutex::new(Known::Detached),
            flags,
        }
    }

    /// No daemon behind it, showing `known` as given: a render's.
    pub fn fixed(known: Known) -> Switches {
        Switches {
            transport: None,
            known: Mutex::new(known),
            flags: None,
        }
    }

    /// The running app's: the daemon's socket in the data folder, or nothing for a fixture root.
    pub fn for_run(root: Option<&DataRoot>) -> Switches {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Switches::over(
                Box::new(Socket {
                    socket: root.data_dir.join(SOCKET_NAME),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                }),
                Some(Flags {
                    file: root.data_dir.join("settings.json"),
                }),
            ),
            None => Switches::fixed(Known::Detached),
        }
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn known(&self) -> Known {
        *lock(&self.known)
    }

    /// Whether the user chose this switch on.
    pub fn chosen(&self, name: &str) -> bool {
        self.flags
            .as_ref()
            .is_some_and(|f| f.get(&choice_key(name)))
    }

    fn set_known(&self, known: Known) {
        *lock(&self.known) = known;
    }

    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let Some(transport) = &self.transport else {
            return Err(ClientError::Closed);
        };
        let result = transport.call(op, args);
        if let Err(e) = &result {
            if acting::after_error(e) == Some(Acting::Detached) {
                self.set_known(Known::Detached);
            }
        }
        result
    }

    /// Reads the switches, and sends the user's choice again for each one chosen on that the daemon
    /// says is off. Blocking.
    pub fn refresh(&self) -> Known {
        if self.transport.is_none() {
            return self.known();
        }
        let Ok(reply) = self.call(OP_GET, json!({})) else {
            return self.known();
        };
        let Some(mut states) = States::parse(&reply) else {
            return self.known();
        };
        for name in NAMES {
            if !states.get(name) && self.chosen(name) {
                match self.call(OP_SET, json!({"switch": name, "on": true})) {
                    Ok(done) if done.get("on").and_then(Value::as_bool) == Some(true) => {
                        states = states.with(name, true);
                    }
                    Ok(_) | Err(_) => {}
                }
            }
        }
        self.set_known(Known::States(states));
        self.known()
    }

    /// The user confirmed: keeps the choice in the app's settings, then tells the daemon. Blocking.
    pub fn choose(&self, name: &str, on: bool) -> Result<(), String> {
        if !NAMES.contains(&name) {
            return Err(format!("no switch named {name}"));
        }
        if self.transport.is_none() {
            return Err(acting::DETACHED_MESSAGE.into());
        }
        let Some(flags) = &self.flags else {
            return Err("there is nowhere to keep the choice".into());
        };
        flags.set(&choice_key(name), on)?;
        match self.call(OP_SET, json!({"switch": name, "on": on})) {
            Ok(_) => {
                let now = match self.known() {
                    Known::States(s) => s,
                    Known::Detached => States::default(),
                };
                self.set_known(Known::States(now.with(name, on)));
                Ok(())
            }
            Err(e) => Err(message(&e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ApiError, ErrorCode};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// A daemon with three switches that start off.
    #[derive(Default)]
    struct FakeDaemon {
        on: Mutex<States>,
        calls: Mutex<Vec<(String, Value)>>,
        down: Mutex<bool>,
        refuse: Mutex<bool>,
    }

    impl Transport for Arc<FakeDaemon> {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((op.to_owned(), args.clone()));
            if *self.down.lock().unwrap() {
                return Err(ClientError::Closed);
            }
            let mut on = self.on.lock().unwrap();
            match op {
                OP_GET => {
                    Ok(json!({"accounts": on.accounts, "vault": on.vault, "install": on.install}))
                }
                OP_SET => {
                    if *self.refuse.lock().unwrap() {
                        return Err(ClientError::Api(ApiError::new(
                            ErrorCode::Forbidden,
                            "app tier only",
                        )));
                    }
                    let name = args["switch"].as_str().unwrap().to_owned();
                    let value = args["on"].as_bool().unwrap();
                    *on = on.with(&name, value);
                    Ok(json!({"switch": name, "on": value}))
                }
                _ => Err(ClientError::Api(ApiError::bad_request("no such op"))),
            }
        }
    }

    fn setup(name: &str) -> (Switches, Arc<FakeDaemon>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nl-switches-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let daemon = Arc::new(FakeDaemon::default());
        let switches = Switches::over(Box::new(daemon.clone()), Some(Flags { file: file.clone() }));
        (switches, daemon, file)
    }

    fn ops_of(d: &FakeDaemon) -> Vec<String> {
        d.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(o, _)| o.clone())
            .collect()
    }

    #[test]
    fn nothing_is_asked_of_the_daemon_until_the_user_chose() {
        let (s, daemon, _) = setup("none");
        assert_eq!(s.known(), Known::Detached);
        assert_eq!(s.refresh(), Known::States(States::default()));
        assert_eq!(ops_of(&daemon), vec![OP_GET], "a read, no set");
    }

    #[test]
    fn a_restarted_daemon_gets_the_choice_again() {
        let (s, daemon, _) = setup("again");
        s.choose(VAULT, true).unwrap();
        s.choose(INSTALL, true).unwrap();
        s.choose(INSTALL, false).unwrap();
        assert!(daemon.on.lock().unwrap().vault);
        // The daemon restarts: every switch is off again.
        *daemon.on.lock().unwrap() = States::default();
        daemon.calls.lock().unwrap().clear();
        let known = s.refresh();
        assert_eq!(
            known,
            Known::States(States {
                accounts: false,
                vault: true,
                install: false
            })
        );
        let sets: Vec<Value> = daemon
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(o, _)| o == OP_SET)
            .map(|(_, a)| a.clone())
            .collect();
        assert_eq!(sets, vec![json!({"switch": "vault", "on": true})]);
        assert!(s.chosen(VAULT) && !s.chosen(INSTALL) && !s.chosen(ACCOUNTS));
    }

    #[test]
    fn a_refused_set_leaves_the_switch_off_and_says_why() {
        let (s, daemon, _) = setup("refused");
        *daemon.refuse.lock().unwrap() = true;
        assert_eq!(s.choose(ACCOUNTS, true), Err("app tier only".to_owned()));
        assert!(
            s.chosen(ACCOUNTS),
            "the choice is kept; the daemon didn't take it"
        );
        assert_eq!(s.refresh(), Known::States(States::default()));
    }

    #[test]
    fn a_lost_daemon_detaches() {
        let (s, daemon, _) = setup("down");
        s.refresh();
        assert!(matches!(s.known(), Known::States(_)));
        *daemon.down.lock().unwrap() = true;
        s.refresh();
        assert_eq!(s.known(), Known::Detached);
        assert!(s.choose(VAULT, true).is_err());
    }

    #[test]
    fn unknown_names_and_no_transport_are_refused_unsent() {
        let (s, daemon, _) = setup("names");
        assert!(s.choose("acting", true).is_err());
        assert!(s.choose("", true).is_err());
        assert!(ops_of(&daemon).is_empty());
        let fixed = Switches::fixed(Known::Detached);
        assert!(!fixed.is_live());
        assert_eq!(
            fixed.choose(VAULT, true),
            Err(acting::DETACHED_MESSAGE.to_owned())
        );
        assert_eq!(fixed.refresh(), Known::Detached);
    }

    #[test]
    fn replies_are_read_tolerantly() {
        assert_eq!(
            States::parse(&json!({"accounts": true})),
            Some(States {
                accounts: true,
                vault: false,
                install: false
            })
        );
        assert_eq!(States::parse(&Value::Null), None);
        assert_eq!(States::parse(&json!([1])), None);
    }
}
