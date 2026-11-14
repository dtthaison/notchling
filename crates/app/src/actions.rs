use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notchling_local_api::{
    Client, ClientError, ClientKind, ConnectOptions, CALL_TIMEOUT, SOCKET_NAME,
};
use serde_json::{json, Value};

use crate::acting::{self, ops, Acting, Request};
use crate::chat::data::DataRoot;
use crate::switches::restart::{Area, RestartNotice, Said};

pub trait Transport: Send + Sync {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError>;

    fn close(&self) {}
}

pub struct Socket {
    pub socket: PathBuf,
    pub version: String,
}

pub const SESSION_CALL_TIMEOUT: Duration = Duration::from_secs(60);

pub fn call_timeout(op: &str) -> Duration {
    if op.starts_with("session.") || op == ops::FOLDER_TRUST {
        SESSION_CALL_TIMEOUT
    } else {
        CALL_TIMEOUT
    }
}

impl Transport for Socket {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let options = ConnectOptions {
            timeout: call_timeout(op),
            ..ConnectOptions::default()
        };
        let mut client =
            Client::connect_with(&self.socket, ClientKind::App, &self.version, &options)?;
        client.call(op, args)
    }
}

pub const ACTING_CHOICE_KEY: &str = "actingOn";

pub trait Choice: Send + Sync {
    fn get(&self) -> bool;
    fn set(&self, on: bool) -> Result<(), String>;
}

struct NoChoice;

impl Choice for NoChoice {
    fn get(&self) -> bool {
        false
    }
    fn set(&self, _on: bool) -> Result<(), String> {
        Err("there is nowhere to keep the choice".into())
    }
}

pub struct SettingsChoice {
    pub file: PathBuf,
}

impl Choice for SettingsChoice {
    fn get(&self) -> bool {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .get(ACTING_CHOICE_KEY)
    }

    fn set(&self, on: bool) -> Result<(), String> {
        crate::welcome::own::Flags {
            file: self.file.clone(),
        }
        .set_choice(ACTING_CHOICE_KEY, on)
    }
}

pub struct Actions {
    transport: Option<Box<dyn Transport>>,
    said: Mutex<Acting>,
    choice: Box<dyn Choice>,
    restart: Option<Arc<RestartNotice>>,
}

impl Actions {
    pub fn over(transport: Box<dyn Transport>) -> Actions {
        Actions {
            transport: Some(transport),
            said: Mutex::new(Acting::Off),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

    pub fn with_choice(mut self, choice: Box<dyn Choice>) -> Actions {
        self.choice = choice;
        self
    }

    pub fn with_restart(mut self, notice: Arc<RestartNotice>) -> Actions {
        self.restart = Some(notice);
        self
    }

    pub fn fixed(acting: Acting) -> Actions {
        Actions {
            transport: None,
            said: Mutex::new(acting),
            choice: Box::new(NoChoice),
            restart: None,
        }
    }

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

    pub fn acting(&self, attached: bool) -> Acting {
        match self.transport {
            Some(_) => acting::effective(attached, self.said()),
            None => self.said(),
        }
    }

    pub fn refresh(&self) -> Acting {
        let Some(transport) = &self.transport else {
            return self.said();
        };
        let now = match transport.call(ops::DAEMON_STATUS, json!({})) {
            Ok(status) => acting::from_status(&status),
            Err(e) => acting::after_error(&e).unwrap_or_else(|| self.said()),
        };
        if let Some(notice) = &self.restart {
            let said = match now {
                Acting::On => Said::On,
                Acting::Off => Said::Off,
                Acting::Detached => Said::Detached,
            };
            notice.observe(Area::Acting, said, self.choice.get());
        }
        self.set(now);
        now
    }

    pub fn choose(&self, on: bool) -> Result<Acting, String> {
        self.choose_as(on, false)
    }

    pub fn choose_as(&self, on: bool, restored: bool) -> Result<Acting, String> {
        let Some(transport) = &self.transport else {
            return Err(Acting::Detached.message().unwrap_or_default().to_owned());
        };
        let args = if restored {
            json!({ "on": on, "restored": true })
        } else {
            json!({ "on": on })
        };
        match transport.call(ops::SETTINGS_SET_ACTING, args) {
            Ok(reply) => {
                let now = acting::from_status(&reply);
                self.set(now);
                if let Some(notice) = &self.restart {
                    notice.chose(Area::Acting, on);
                }
                if let Err(e) = self.choice.set(on) {
                    eprintln!("acting: the choice isn't kept ({e})");
                }
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

    pub fn read(&self, request: &Request) -> Result<Value, String> {
        let Some(transport) = &self.transport else {
            return Err(Acting::Detached.message().unwrap_or_default().to_owned());
        };
        transport
            .call(request.op, request.args.clone())
            .map_err(|e| {
                if let Some(Acting::Detached) = acting::after_error(&e) {
                    self.set(Acting::Detached);
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
    use crate::switches::restart::{Area, RestartNotice};
    use notchling_local_api::{ApiError, ErrorCode};
    use std::sync::Arc;

    #[test]
    fn calls_that_type_into_a_session_wait_longer_than_the_daemon_does() {
        for op in [
            ops::SESSION_REPLY,
            ops::SESSION_ANSWER,
            ops::SESSION_PERMISSION,
            ops::SESSION_INTERRUPT,
            ops::SESSION_TAKE_BACK,
            ops::SESSION_COMMAND,
            ops::SESSION_NEW,
            ops::FOLDER_TRUST,
            ops::SESSION_MOVE,
            ops::SESSION_STOP,
            ops::SESSION_REMOVE,
        ] {
            assert_eq!(call_timeout(op), SESSION_CALL_TIMEOUT, "{op}");
        }
        assert!(SESSION_CALL_TIMEOUT >= std::time::Duration::from_secs(60));
        assert_eq!(
            call_timeout(ops::DAEMON_STATUS),
            notchling_local_api::CALL_TIMEOUT
        );
        assert_eq!(
            call_timeout(ops::SETTINGS_SET_ACTING),
            notchling_local_api::CALL_TIMEOUT
        );
    }

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;
    type Script = Box<dyn Fn(&str) -> Result<Value, ClientError> + Send + Sync>;

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
        assert_eq!(actions.acting(false), Acting::Detached);
        let err = actions.send(&Request::reply("s1", "hi")).unwrap_err();
        assert_eq!(err, OFF_MESSAGE);
        assert_eq!(actions.acting(true), Acting::Off);
        let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
        assert_eq!(sent, ["daemon.status", "session.reply"]);
    }

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
    fn a_remembered_choice_never_turns_acting_on_it_only_offers_the_notice() {
        let (actions, calls) = fake(|op| match op {
            "daemon.status" => Ok(json!({"acting": false})),
            _ => Ok(json!({"acting": true})),
        });
        let notice = Arc::new(RestartNotice::default());
        let actions = actions
            .with_choice(Box::new(Chosen(Mutex::new(true))))
            .with_restart(notice.clone());
        assert_eq!(actions.refresh(), Acting::Off);
        assert_eq!(actions.refresh(), Acting::Off);
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![
                ("daemon.status".to_owned(), json!({})),
                ("daemon.status".to_owned(), json!({}))
            ]
        );
        assert_eq!(notice.state().offer(), [Area::Acting]);

        let (actions, calls) = fake(|_| Ok(json!({"acting": true})));
        let chosen = Arc::new(Chosen(Mutex::new(false)));
        let actions = actions
            .with_choice(Box::new(SharedChoice(chosen.clone())))
            .with_restart(notice.clone());
        assert_eq!(actions.choose(true), Ok(Acting::On));
        assert!(chosen.get());
        assert_eq!(notice.state().offer(), []);
        assert_eq!(
            calls.lock().unwrap().last().cloned(),
            Some(("settings.set_acting".to_owned(), json!({"on": true})))
        );
    }

    struct Broken;

    impl Choice for Broken {
        fn get(&self) -> bool {
            true
        }
        fn set(&self, _: bool) -> Result<(), String> {
            Err("settings file can't be read".into())
        }
    }

    #[test]
    fn off_reaches_the_daemon_even_when_the_choice_cant_be_kept() {
        let (actions, calls) = fake(|_| Ok(json!({"acting": false})));
        let actions = actions.with_choice(Box::new(Broken));
        assert_eq!(actions.choose(false), Ok(Acting::Off));
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![("settings.set_acting".to_owned(), json!({"on": false}))]
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
        let kept = notchling_engine::settings::Settings::open(&file).unwrap();
        assert_eq!(kept.raw(ACTING_CHOICE_KEY), Some(&json!(true)));
        assert!(!file.exists(), "kept in notchling.db, not in the old file");
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
    fn a_read_needs_no_acting_and_a_refusal_changes_nothing() {
        let (actions, calls) = fake(|op| match op {
            "agy.conversation" => Ok(json!({"open": false})),
            _ => Err(ClientError::Api(ApiError::bad_request("no such session"))),
        });
        let read = Request {
            op: "agy.conversation",
            args: json!({"id": "agy:s1", "peek": true}),
        };
        assert_eq!(actions.acting(true), Acting::Off);
        assert_eq!(actions.read(&read).unwrap(), json!({"open": false}));
        assert_eq!(calls.lock().unwrap().len(), 1, "sent, acting off or not");
        let bad = Request {
            op: "agy.state",
            args: json!({"id": "x"}),
        };
        assert_eq!(actions.read(&bad).unwrap_err(), "no such session");
        assert_eq!(actions.acting(true), Acting::Off);
        let (gone, _) = fake(|_| Err(ClientError::Closed));
        assert!(gone.read(&read).is_err());
        assert_eq!(gone.acting(true), Acting::Detached);
        assert!(Actions::fixed(Acting::On).read(&read).is_err());
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
