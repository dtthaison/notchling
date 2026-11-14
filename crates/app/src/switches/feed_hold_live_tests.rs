use super::*;
use crate::testutil::Scratch;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, Tier,
};
use std::path::Path;

struct Daemon {
    holding: Mutex<Value>,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SETTINGS_SWITCHES => Some(OpSpec::AGENT),
            ops::VAULT_PENDING | ops::AWAKE_STATE => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SETTINGS_SWITCHES => {
                Ok(json!({"accounts": false, "vault": false, "install": false}))
            }
            ops::VAULT_PENDING => Ok(json!({"approvals": [], "requests": []})),
            ops::AWAKE_STATE => Ok(self.holding.lock().unwrap().clone()),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }

    fn topics(&self) -> &[&str] {
        &["settings", "accounts", "providers", "limits", "vault"]
    }

    fn topic_tier(&self, _: &str) -> Tier {
        Tier::App
    }
}

struct Direct {
    socket: std::path::PathBuf,
}

impl Transport for Direct {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let options = ConnectOptions {
            check_server: false,
            ..ConnectOptions::default()
        };
        Client::connect_with(&self.socket, ClientKind::App, "test", &options)?.call(op, args)
    }
}

fn start(socket: &Path) -> SwitchFeed {
    let config = FeedConfig {
        socket: socket.to_path_buf(),
        version: "test".into(),
        check_server: false,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
    };
    SwitchFeed::start(
        config,
        Arc::new(Direct {
            socket: socket.to_path_buf(),
        }),
        Arc::new(NoChoice),
        Arc::default(),
    )
}

fn wait_until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < end, "waited for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_cup_follows_what_the_daemon_holds_from_the_first_read_through_each_event_to_its_loss() {
    let scratch = Scratch::new("feed-hold");
    let socket = scratch.path().join(SOCKET_NAME);
    let daemon = Arc::new(Daemon {
        holding: Mutex::new(json!({"holding": "display"})),
    });
    let (handle, thread) = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            audit: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        daemon.clone(),
    )
    .expect("bind")
    .spawn()
    .expect("serve");
    let feed = start(&socket);
    wait_until("the first read", || feed.take_hold() == Some(true));
    wait_until("the follow", || handle.app_following("settings"));
    assert_eq!(feed.take_hold(), None);
    handle.publish("settings", "awake.changed", &json!({"holding": null}));
    wait_until("the release", || feed.take_hold() == Some(false));
    handle.publish("settings", "awake.changed", &json!({"holding": "system"}));
    wait_until("the hold", || feed.take_hold() == Some(true));
    handle.shutdown();
    let _ = thread.join();
    wait_until("the loss", || feed.take_hold() == Some(false));
}

#[test]
fn a_daemon_that_does_not_know_the_op_leaves_the_cup_empty() {
    let scratch = Scratch::new("feed-hold-old");
    let socket = scratch.path().join(SOCKET_NAME);
    struct Old;
    impl Handler for Old {
        fn spec(&self, op: &str) -> Option<OpSpec> {
            match op {
                ops::SETTINGS_SWITCHES => Some(OpSpec::AGENT),
                ops::VAULT_PENDING => Some(OpSpec::APP),
                _ => None,
            }
        }
        fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
            match op {
                ops::SETTINGS_SWITCHES => {
                    Ok(json!({"accounts": false, "vault": false, "install": false}))
                }
                _ => Ok(json!({"approvals": [], "requests": []})),
            }
        }
        fn topics(&self) -> &[&str] {
            &["settings", "accounts", "providers", "limits", "vault"]
        }
        fn topic_tier(&self, _: &str) -> Tier {
            Tier::App
        }
    }
    let (handle, thread) = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            audit: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Old),
    )
    .expect("bind")
    .spawn()
    .expect("serve");
    let feed = start(&socket);
    wait_until("the follow", || feed.following());
    assert_eq!(feed.take_hold(), None);
    handle.shutdown();
    let _ = thread.join();
}
