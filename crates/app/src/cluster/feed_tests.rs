use std::path::Path;
use std::thread::JoinHandle;

use notchling_local_api::{
    ops, Allowlist, ApiError, Caller, ErrorCode, Handler, Limits, OpSpec, Server, ServerConfig,
    ServerHandle, Tier, SOCKET_NAME,
};
use serde_json::{json, Value};

use super::feed::{carried, now_ms, ClusterFeed};
use super::renders::{moving, replication, status, ME, STANDBY};
use super::wire;
use crate::actions::Transport;
use crate::session_source::Backoff;
use crate::switches::feed::FeedConfig;
use crate::testutil::Scratch;
use notchling_engine::versions::Tool;
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
            ops::CLUSTER_STATUS
            | ops::CLUSTER_JOIN_TOKEN
            | ops::CLUSTER_JOIN
            | ops::SETTINGS_SET_SWITCH => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args.clone()));
        if op == ops::CLUSTER_JOIN {
            let token = args["token"].as_str().unwrap_or("");
            return if token.ends_with("refused") {
                Err(ApiError::new(
                    ErrorCode::Failed,
                    format!("the primary refused {token}"),
                ))
            } else {
                Ok(json!({"state": "pending"}))
            };
        }
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

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn a_join_goes_to_the_daemon_and_an_error_never_carries_the_token() {
    let scratch = Scratch::new("cl-join");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let (handle, thread) = serve(&socket, &calls);
    let feed = start(&socket);
    wait_until("the status", || feed.snapshot().is_loaded());
    let wire::JoinInput::Send(request) = wire::join("nlj1.goodtoken") else {
        panic!("a token is sent");
    };
    assert_eq!(feed.send(&request), Ok(json!({"state": "pending"})));
    assert!(recorded(&calls).contains(&(
        ops::CLUSTER_JOIN.to_owned(),
        json!({"token": "nlj1.goodtoken"})
    )));
    let wire::JoinInput::Send(request) = wire::join("nlj1.SECRETrefused") else {
        panic!("a token is sent");
    };
    let error = feed.send(&request).expect_err("the daemon refuses");
    assert!(error.contains("the primary refused"), "{error}");
    assert!(!error.contains("SECRETrefused"), "{error}");
    drop(feed);
    handle.shutdown();
    let _ = thread.join();
}

#[test]
fn the_feed_stamps_a_tool_or_login_only_when_it_changes_after_first_sight() {
    let with = |codex: &str, claude: &str, clock: &str| {
        let mut next = status();
        let member = next["members"]
            .as_array_mut()
            .and_then(|m| m.iter_mut().find(|m| m["id"] == STANDBY))
            .expect("standby");
        member["versions"]["tools"] = json!({"codex": codex, "claude": claude});
        member["versions"]["notchling"] = json!(clock);
        next
    };
    let feed = ClusterFeed::none();
    assert_eq!(feed.reported_at(STANDBY, Tool::Codex), None);
    feed.take_status(&with("0.118.0", "2.1.0", "1"));
    assert_eq!(feed.reported_at(STANDBY, Tool::Codex), Some(0));
    assert_eq!(feed.reported_at(STANDBY, Tool::Claude), Some(0));
    feed.take_status(&with("0.118.0", "2.1.0", "2"));
    assert_eq!(feed.reported_at(STANDBY, Tool::Codex), Some(0));
    let before = now_ms();
    feed.take_status(&with("0.166.0", "2.1.0", "2"));
    assert!(feed
        .reported_at(STANDBY, Tool::Codex)
        .is_some_and(|at| at >= before));
    assert_eq!(feed.reported_at(STANDBY, Tool::Claude), Some(0));
    assert_eq!(feed.reported_at("nobody", Tool::Codex), None);
}

#[test]
fn an_event_keeps_the_readiness_and_set_facts_it_does_not_carry() {
    let last = replication();
    let mut event = status();
    for member in event["members"].as_array_mut().expect("members") {
        member.as_object_mut().expect("member").remove("ready");
    }
    event.as_object_mut().expect("status").remove("replication");
    let next = carried(&last, &event);
    assert_eq!(next["replication"], last["replication"]);
    let ready = |status: &Value, id: &str| {
        status["members"]
            .as_array()
            .and_then(|m| m.iter().find(|m| m["id"] == id))
            .map(|m| m["ready"].clone())
    };
    assert_eq!(ready(&next, STANDBY), ready(&last, STANDBY));
    assert_eq!(ready(&next, ME), Some(Value::Null));
    let running = moving("freeze");
    let again = carried(&running, &event);
    assert_eq!(again["move"], running["move"]);
    let fresh = carried(&Value::Null, &event);
    assert!(fresh.get("replication").is_none());
}

#[cfg(unix)]
#[test]
fn a_feed_with_no_daemon_says_it_is_down_and_recovers_when_one_answers() {
    let scratch = Scratch::new("cf-down");
    let socket = scratch.path().join(SOCKET_NAME);
    let feed = start(&socket);
    assert!(feed.is_live());
    wait_until("the feed to notice no daemon", || feed.is_down());
    assert!(!feed.snapshot().is_loaded());
    let calls: Calls = Arc::default();
    let (handle, thread) = serve(&socket, &calls);
    wait_until("the status", || feed.snapshot().is_loaded());
    assert!(!feed.is_down());
    handle.shutdown();
    thread.join().expect("the server ends").ok();
}
