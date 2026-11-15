use std::path::Path;
use std::thread::JoinHandle;

use notchling_local_api::{
    ops, Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle,
    Tier, SOCKET_NAME,
};
use serde_json::{json, Value};

use super::feed::ClusterFeed;
use super::renders::{status, ME};
use super::wire;
use crate::actions::Transport;
use crate::session_source::Backoff;
use crate::switches::feed::FeedConfig;
use crate::testutil::Scratch;
use notchling_local_api::{Client, ClientError, ClientKind, ConnectOptions};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Daemon {
    calls: Calls,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::CLUSTER_STATUS | ops::CLUSTER_JOIN_TOKEN | ops::SETTINGS_SET_SWITCH => {
                Some(OpSpec::APP)
            }
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args.clone()));
        Ok(match op {
            ops::CLUSTER_STATUS => status(),
            ops::CLUSTER_JOIN_TOKEN => json!({"token": "nlj1.secret"}),
            _ => json!({"switch": "cluster", "on": true}),
        })
    }

    fn topics(&self) -> &[&str] {
        &["cluster"]
    }

    fn topic_tier(&self, _: &str) -> Tier {
        Tier::Agent
    }
}

fn serve(socket: &Path, calls: &Calls) -> Running {
    Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            events: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon {
            calls: calls.clone(),
        }),
    )
    .expect("bind")
    .spawn()
    .expect("serve")
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

fn start(socket: &Path) -> ClusterFeed {
    let config = FeedConfig {
        socket: socket.to_path_buf(),
        version: "test".into(),
        check_server: false,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
    };
    ClusterFeed::start(
        config,
        Arc::new(Direct {
            socket: socket.to_path_buf(),
        }),
    )
}

fn wait_until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < end, "waited for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn recorded(calls: &Calls) -> Vec<(String, Value)> {
    calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[test]
fn the_feed_reads_the_status_and_turns_the_switch_on_only_when_asked() {
    let scratch = Scratch::new("cluster-feed");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let (handle, thread) = serve(&socket, &calls);
    let feed = start(&socket);
    wait_until("the status", || feed.snapshot().is_loaded());
    assert_eq!(feed.snapshot().rows().len(), 3);
    assert_eq!(feed.snapshot().me(), ME);
    assert!(
        recorded(&calls)
            .iter()
            .all(|(op, _)| op == ops::CLUSTER_STATUS),
        "reading the cluster asks nothing else: {:?}",
        recorded(&calls)
    );
    let mut moved = status();
    moved["members"][0]["online"] = json!(true);
    handle.publish("cluster", "cluster.changed", &moved);
    wait_until("the event", || {
        feed.snapshot().rows().iter().all(|r| r.online)
    });
    feed.send(&wire::turn_on()).expect("turn on");
    assert!(recorded(&calls).contains(&(
        ops::SETTINGS_SET_SWITCH.to_owned(),
        json!({"switch": "cluster", "on": true})
    )));
    assert_eq!(feed.token().as_deref(), Ok("nlj1.secret"));
    drop(feed);
    handle.shutdown();
    let _ = thread.join();
}

#[test]
fn without_a_daemon_the_model_is_empty_and_a_call_says_so() {
    let feed = ClusterFeed::none();
    assert!(!feed.is_live());
    assert!(!feed.snapshot().is_loaded());
    assert_eq!(
        feed.send(&wire::turn_on()),
        Err(crate::acting::DETACHED_MESSAGE.to_owned())
    );
    assert!(feed.token().is_err());
}
