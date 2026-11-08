//! Sending actions to the daemon: one blocking call per action over the local API's generic
//! `Client::call`, made off the UI thread by the caller. [`Actions`] keeps what the daemon last said
//! about acting, so every control can be drawn from [`Actions::acting`] without a socket read.
//!
//! Nothing is sent while acting isn't on: [`Actions::send`] refuses locally first, so a control
//! that slipped past its disabled state still can't reach a session. A render or a fixture root has
//! no transport at all ([`Actions::fixed`], [`Actions::for_run`]).
//!
//! Every daemon run starts with acting off, whatever its files say (decision 0007's note of
//! 2026-11-08). The user's choice is the app's to keep ([`Choice`], `actingOn` in the app's own
//! `settings.json`): when the daemon says off and the user chose on, [`Actions::refresh`] sends
//! `settings.set_acting` again, so a restarted daemon comes back to what the user chose.

use std::path::PathBuf;
use std::sync::Mutex;

use notchling_engine::settings::Settings;

use notchling_local_api::{Client, ClientError, ClientKind, ConnectOptions, SOCKET_NAME};
use serde_json::{json, Value};

use crate::acting::{self, ops, Acting, Request};
use crate::chat::data::DataRoot;

/// One call to the daemon; the socket in the app, a fake in tests.
pub trait Transport: Send + Sync {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError>;
}

/// The real transport: connect (checking who serves the socket), hello as the app, one call.
pub struct Socket {
    pub socket: PathBuf,
    pub version: String,
}

impl Transport for Socket {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let options = ConnectOptions::default();
        let mut client =
            Client::connect_with(&self.socket, ClientKind::App, &self.version, &options)?;
        client.call(op, args)
    }
}

/// The key in the app's `settings.json` that holds the user's choice. Not in the engine's key list
/// yet (that belongs to `engine::settings`), so it is written past the store's key check.
pub const ACTING_CHOICE_KEY: &str = "actingOn";

/// Where the user's choice of acting is kept.
pub trait Choice: Send + Sync {
    fn get(&self) -> bool;
    fn set(&self, on: bool) -> Result<(), String>;
}

/// No choice: off. A render's, and the default.
struct NoChoice;

impl Choice for NoChoice {
    fn get(&self) -> bool {
        false
    }
    fn set(&self, _on: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

/// The choice in the app's own settings file (the one the Settings sheet writes).
pub struct SettingsChoice {
    pub file: PathBuf,
}

impl Choice for SettingsChoice {
    fn get(&self) -> bool {
        Settings::open(&self.file)
            .ok()
            .and_then(|s| s.raw(ACTING_CHOICE_KEY).and_then(Value::as_bool))
            .unwrap_or(false)
    }

    fn set(&self, on: bool) -> Result<(), String> {
        let mut store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        let mut values = store.values().clone();
        values.insert(ACTING_CHOICE_KEY.into(), Value::Bool(on));
        store.replace_all(values).map_err(|e| e.to_string())
    }
}

/// The app's actions and what it knows about acting.
pub struct Actions {
    transport: Option<Box<dyn Transport>>,
    said: Mutex<Acting>,
    choice: Box<dyn Choice>,
}

impl Actions {
    /// Over a transport; acting is off until the daemon says otherwise.
    pub fn over(transport: Box<dyn Transport>) -> Actions {
        Actions {
            transport: Some(transport),
            said: Mutex::new(Acting::Off),
            choice: Box::new(NoChoice),
        }
    }

    /// Keeps the user's choice in `choice`.
    pub fn with_choice(mut self, choice: Box<dyn Choice>) -> Actions {
        self.choice = choice;
        self
    }

    /// No daemon behind it, showing `acting` as given: a render's.
    pub fn fixed(acting: Acting) -> Actions {
        Actions {
            transport: None,
            said: Mutex::new(acting),
            choice: Box::new(NoChoice),
        }
    }

    /// The running app's: the daemon's socket in the data folder, or nothing for a fixture root,
    /// which never touches a daemon.
    pub fn for_run(root: Option<&DataRoot>) -> Actions {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Actions::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }))
            .with_choice(Box::new(SettingsChoice {
                file: root.data_dir.join("settings.json"),
            })),
            None => Actions::fixed(Acting::Detached),
        }
    }

    fn said(&self) -> Acting {
        *self.said.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, acting: Acting) {
        *self.said.lock().unwrap_or_else(|e| e.into_inner()) = acting;
    }

    /// What every action control follows. `attached` is whether the session rows come from the
    /// daemon right now; without a transport it is what [`Actions::fixed`] was given.
    pub fn acting(&self, attached: bool) -> Acting {
        match self.transport {
            Some(_) => acting::effective(attached, self.said()),
            None => self.said(),
        }
    }

    /// Asks the daemon whether acting is on (`daemon.status`). Blocking.
    pub fn refresh(&self) -> Acting {
        let Some(transport) = &self.transport else {
            return self.said();
        };
        let mut now = match transport.call(ops::DAEMON_STATUS, json!({})) {
            Ok(status) => acting::from_status(&status),
            Err(e) => acting::after_error(&e).unwrap_or_else(|| self.said()),
        };
        // A daemon that started since (they all start off): the user's choice again.
        if now == Acting::Off && self.choice.get() {
            now = match transport.call(ops::SETTINGS_SET_ACTING, json!({"on": true})) {
                Ok(reply) => acting::from_status(&reply),
                Err(e) => acting::after_error(&e).unwrap_or(Acting::Off),
            };
        }
        self.set(now);
        now
    }

    /// The user chose: keeps it in the app's settings, then tells the daemon. Blocking.
    pub fn choose(&self, on: bool) -> Result<Acting, String> {
        let Some(transport) = &self.transport else {
            return Err(Acting::Detached.message().unwrap_or_default().to_owned());
        };
        self.choice.set(on)?;
        match transport.call(ops::SETTINGS_SET_ACTING, json!({ "on": on })) {
            Ok(reply) => {
                let now = acting::from_status(&reply);
                self.set(now);
                Ok(now)
            }
            Err(e) => {
                if let Some(acting) = acting::after_error(&e) {
                    self.set(acting);
                }
                Err(e.to_string())
            }
        }
    }

    /// Sends one action. Blocking. Refused here, unsent, unless acting is on; a refusal from the
    /// daemon updates what is known about acting. The error is the line to show.
    pub fn send(&self, request: &Request) -> Result<Value, String> {
        let said = self.said();
        let Some(transport) = &self.transport else {
            return Err(Acting::Detached.message().unwrap_or_default().to_owned());
        };
        if let Some(message) = said.message() {
            return Err(message.to_owned());
        }
        transport
            .call(request.op, request.args.clone())
            .map_err(|e| {
                if let Some(acting) = acting::after_error(&e) {
                    self.set(acting);
                }
                match e {
                    ClientError::Api(api) => api.message,
                    other => other.to_string(),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acting::{Decision, OFF_MESSAGE};
    use notchling_local_api::{ApiError, ErrorCode};
    use std::sync::Arc;

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;
    type Script = Box<dyn Fn(&str) -> Result<Value, ClientError> + Send + Sync>;

    /// Answers each op from a script and records every call.
    struct Fake {
        calls: Calls,
        answer: Script,
    }

    impl Transport for Fake {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls.lock().unwrap().push((op.to_owned(), args));
            (self.answer)(op)
        }
    }

    fn fake(
        answer: impl Fn(&str) -> Result<Value, ClientError> + Send + Sync + 'static,
    ) -> (Actions, Calls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let actions = Actions::over(Box::new(Fake {
            calls: calls.clone(),
            answer: Box::new(answer),
        }));
        (actions, calls)
    }

    #[test]
    fn nothing_is_sent_until_the_daemon_says_acting_is_on() {
        let (actions, calls) = fake(|_| Ok(json!({"version": "0.1.0"})));
        assert_eq!(actions.acting(true), Acting::Off);
        let err = actions
            .send(&Request::permission("s1", Decision::Allow))
            .unwrap_err();
        assert_eq!(err, OFF_MESSAGE);
        assert!(calls.lock().unwrap().is_empty());
        // A status without `acting` keeps it off.
        assert_eq!(actions.refresh(), Acting::Off);
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert!(actions.send(&Request::stop("s1")).is_err());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn on_sends_and_the_off_refusal_turns_it_off_again() {
        let (actions, calls) = fake(|op| match op {
            "daemon.status" => Ok(json!({"acting": true})),
            _ => Err(ClientError::Api(ApiError::new(
                ErrorCode::Forbidden,
                OFF_MESSAGE,
            ))),
        });
        assert_eq!(actions.refresh(), Acting::On);
        assert_eq!(actions.acting(true), Acting::On);
        // Rows read from the files mean no daemon: detached, whatever it said.
        assert_eq!(actions.acting(false), Acting::Detached);
        let err = actions.send(&Request::reply("s1", "hi")).unwrap_err();
        assert_eq!(err, OFF_MESSAGE);
        assert_eq!(actions.acting(true), Acting::Off);
        let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
        assert_eq!(sent, ["daemon.status", "session.reply"]);
    }

    /// The user's choice, in memory.
    struct Chosen(Mutex<bool>);

    impl Choice for Chosen {
        fn get(&self) -> bool {
            *self.0.lock().unwrap()
        }
        fn set(&self, on: bool) -> Result<(), String> {
            *self.0.lock().unwrap() = on;
            Ok(())
        }
    }

    #[test]
    fn the_users_choice_is_asserted_again_when_the_daemon_says_off() {
        // Every daemon run starts off; the app turns it back on when the user chose on.
        let daemon_on = Arc::new(Mutex::new(false));
        let on = daemon_on.clone();
        let (actions, calls) = fake(move |op| match op {
            "daemon.status" => Ok(json!({"acting": *on.lock().unwrap()})),
            "settings.set_acting" => {
                *on.lock().unwrap() = true;
                Ok(json!({"acting": true}))
            }
            _ => Ok(json!({})),
        });
        let actions = actions.with_choice(Box::new(Chosen(Mutex::new(true))));
        assert_eq!(actions.refresh(), Acting::On);
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![
                ("daemon.status".to_owned(), json!({})),
                ("settings.set_acting".to_owned(), json!({"on": true}))
            ]
        );
        // Already on: nothing more is sent.
        assert_eq!(actions.refresh(), Acting::On);
        assert_eq!(calls.lock().unwrap().len(), 3);

        // The user chose off: an off daemon stays off, and nothing is asserted.
        let (actions, calls) = fake(|_| Ok(json!({"acting": false})));
        let actions = actions.with_choice(Box::new(Chosen(Mutex::new(false))));
        assert_eq!(actions.refresh(), Acting::Off);
        assert_eq!(calls.lock().unwrap().len(), 1);

        // Choosing sends it and keeps it.
        let (actions, calls) = fake(|_| Ok(json!({"acting": true})));
        let chosen = Arc::new(Chosen(Mutex::new(false)));
        let actions = actions.with_choice(Box::new(SharedChoice(chosen.clone())));
        assert_eq!(actions.choose(true), Ok(Acting::On));
        assert!(chosen.get());
        assert_eq!(
            calls.lock().unwrap().last().cloned(),
            Some(("settings.set_acting".to_owned(), json!({"on": true})))
        );
    }

    struct SharedChoice(Arc<Chosen>);

    impl Choice for SharedChoice {
        fn get(&self) -> bool {
            self.0.get()
        }
        fn set(&self, on: bool) -> Result<(), String> {
            self.0.set(on)
        }
    }

    #[test]
    fn the_choice_is_kept_in_the_apps_own_settings() {
        let dir = std::env::temp_dir().join(format!("nl-app-choice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let choice = SettingsChoice { file: file.clone() };
        assert!(!choice.get(), "off until chosen");
        choice.set(true).unwrap();
        assert!(SettingsChoice { file: file.clone() }.get());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(ACTING_CHOICE_KEY), "{text}");
        choice.set(false).unwrap();
        assert!(!choice.get());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_daemon_that_went_away_detaches() {
        let (actions, _) = fake(|_| Err(ClientError::Closed));
        assert_eq!(actions.refresh(), Acting::Detached);
        assert!(actions.send(&Request::stop("s1")).is_err());
    }

    #[test]
    fn a_fixed_one_never_sends() {
        let actions = Actions::fixed(Acting::On);
        assert_eq!(actions.acting(false), Acting::On);
        assert!(actions.send(&Request::stop("s1")).is_err());
        assert_eq!(actions.refresh(), Acting::On);
        let detached = Actions::for_run(None);
        assert_eq!(detached.acting(true), Acting::Detached);
        let fixture = DataRoot::for_render(None);
        assert_eq!(
            Actions::for_run(Some(&fixture)).acting(true),
            Acting::Detached
        );
    }
}
