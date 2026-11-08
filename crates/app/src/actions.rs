//! Sending actions to the daemon: one blocking call per action over the local API's generic
//! `Client::call`, made off the UI thread by the caller. [`Actions`] keeps what the daemon last said
//! about acting, so every control can be drawn from [`Actions::acting`] without a socket read.
//!
//! Nothing is sent while acting isn't on: [`Actions::send`] refuses locally first, so a control
//! that slipped past its disabled state still can't reach a session. A render or a fixture root has
//! no transport at all ([`Actions::fixed`], [`Actions::for_run`]).

use std::path::PathBuf;
use std::sync::Mutex;

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

/// The app's actions and what it knows about acting.
pub struct Actions {
    transport: Option<Box<dyn Transport>>,
    said: Mutex<Acting>,
}

impl Actions {
    /// Over a transport; acting is off until the daemon says otherwise.
    pub fn over(transport: Box<dyn Transport>) -> Actions {
        Actions {
            transport: Some(transport),
            said: Mutex::new(Acting::Off),
        }
    }

    /// No daemon behind it, showing `acting` as given: a render's.
    pub fn fixed(acting: Acting) -> Actions {
        Actions {
            transport: None,
            said: Mutex::new(acting),
        }
    }

    /// The running app's: the daemon's socket in the data folder, or nothing for a fixture root,
    /// which never touches a daemon.
    pub fn for_run(root: Option<&DataRoot>) -> Actions {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Actions::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
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
        let now = match transport.call(ops::DAEMON_STATUS, json!({})) {
            Ok(status) => acting::from_status(&status),
            Err(e) => acting::after_error(&e).unwrap_or_else(|| self.said()),
        };
        self.set(now);
        now
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
