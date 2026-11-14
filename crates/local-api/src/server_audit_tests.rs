#![cfg(unix)]

use crate::client::{Client, ConnectOptions};
use crate::server::{Caller, Handler, Limits, OpHome, OpSpec, Server, ServerConfig, ServerHandle};
use crate::testutil::{summary, TempDir};
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode, Tier};
use notchling_events::MemorySink;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Acting;

impl Handler for Acting {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "session.reply" | "session.stop" => Some(OpSpec::APP),
            "slow" | "read" => Some(OpSpec::AGENT),
            "peek" | "peek.fail" | "peek.loud" => Some(OpSpec::APP_QUIET),
            _ => None,
        }
    }

    fn loud(&self, op: &str, args: &Value) -> bool {
        op == "peek.loud" && args["refresh"] == json!(true)
    }

    fn home(&self, op: &str) -> OpHome {
        match op {
            "peek" | "peek.fail" | "peek.loud" => OpHome::Quiet,
            "read" | "slow" => OpHome::Quiet,
            _ => OpHome::Called,
        }
    }

    fn known_ops(&self) -> &[&'static str] {
        &[
            "session.reply",
            "session.stop",
            "slow",
            "read",
            "peek",
            "peek.fail",
            "peek.loud",
        ]
    }

    fn topics(&self) -> &[&str] {
        &["sessions", "bridge"]
    }

    fn topic_tier(&self, topic: &str) -> Tier {
        if topic == "bridge" {
            Tier::App
        } else {
            Tier::Agent
        }
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            "session.stop" => Err(ApiError::new(ErrorCode::Unimplemented, "not yet")),
            "peek.fail" => Err(ApiError::new(ErrorCode::Failed, "no")),
            "slow" => {
                std::thread::sleep(Duration::from_millis(400));
                Ok(json!("late"))
            }
            _ => Ok(json!({"sent": true})),
        }
    }
}

struct Running {
    _dir: TempDir,
    socket: PathBuf,
    sink: Arc<MemorySink>,
    handle: ServerHandle,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

fn start(allow: Allowlist) -> Running {
    let dir = TempDir::new("srv-m3");
    let socket = dir.path().join("run/notchlingd.sock");
    let sink = Arc::new(MemorySink::new());
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: allow,
            events: Some(sink.clone()),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Acting),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    Running {
        _dir: dir,
        socket,
        sink,
        handle,
    }
}

fn connect(run: &Running, kind: ClientKind) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(&run.socket, kind, "0.1.0", &options).unwrap()
}

fn calls(run: &Running) -> Vec<(String, String)> {
    summary(&run.sink)
        .into_iter()
        .filter(|(name, _)| name != "app.attached")
        .collect()
}

#[test]
fn app_tier_calls_are_recorded_with_their_outcome() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    let mut app = connect(&run, ClientKind::App);
    assert_eq!(
        app.call("session.reply", json!({})).unwrap(),
        json!({"sent": true})
    );
    assert!(app.call("session.stop", json!({})).is_err());
    assert_eq!(app.call("read", json!({})).unwrap(), json!({"sent": true}));
    assert_eq!(
        calls(&run),
        vec![
            ("op.called".to_owned(), "session.reply".to_owned()),
            ("op.failed".to_owned(), "session.stop".to_owned()),
        ],
        "quiet reads aren't recorded"
    );
}

#[test]
fn a_quiet_op_records_nothing_on_success_but_failures_stay_recorded() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("peek", json!({})).is_ok());
    assert!(app.call("peek.fail", json!({})).is_err());
    let agent_run = start(Allowlist::empty());
    let mut agent = connect(&agent_run, ClientKind::Cli);
    assert!(agent.call("peek", json!({})).is_err());
    assert_eq!(
        calls(&run),
        vec![("op.failed".to_owned(), "peek.fail".to_owned())]
    );
    assert_eq!(
        calls(&agent_run),
        vec![("api.refused".to_owned(), "peek".to_owned())]
    );
}

#[test]
fn a_quiet_op_the_handler_calls_loud_is_recorded_on_success() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("peek.loud", json!({"refresh": false})).is_ok());
    assert!(app.call("peek.loud", json!({"refresh": true})).is_ok());
    assert_eq!(
        calls(&run),
        vec![("op.called".to_owned(), "peek.loud".to_owned())]
    );
}

#[test]
fn the_daemon_knows_when_the_app_is_attached() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    assert!(!run.handle.app_attached());
    let _hook = connect(&run, ClientKind::Hook);
    let _cli = connect(&run, ClientKind::Cli);
    assert!(!run.handle.app_attached());
    let app = connect(&run, ClientKind::App);
    assert!(run.handle.app_attached());
    drop(app);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while run.handle.app_attached() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!run.handle.app_attached());
    let agent_run = start(Allowlist::empty());
    let _posing = connect(&agent_run, ClientKind::App);
    assert!(!agent_run.handle.app_attached());
}

#[test]
fn the_daemon_knows_when_the_app_follows_a_topic() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    assert!(!run.handle.app_following("bridge"));
    let mut app = connect(&run, ClientKind::App);
    assert!(run.handle.app_attached());
    assert!(!run.handle.app_following("bridge"));
    app.subscribe(&["sessions"]).unwrap();
    assert!(!run.handle.app_following("bridge"));
    app.subscribe(&["bridge"]).unwrap();
    assert!(run.handle.app_following("bridge"));
    assert!(!run.handle.app_following("prompts"));
    drop(app);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while run.handle.app_following("bridge") && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!run.handle.app_following("bridge"), "gone with its client");
    let agent_run = start(Allowlist::empty());
    let mut posing = connect(&agent_run, ClientKind::App);
    assert!(posing.subscribe(&["bridge"]).is_err());
    assert!(!agent_run.handle.app_following("bridge"));
}

#[test]
fn a_client_can_wait_longer_for_one_call() {
    let run = start(Allowlist::empty());
    let options = ConnectOptions {
        timeout: Duration::from_millis(100),
        check_server: false,
    };
    let mut c = Client::connect_with(&run.socket, ClientKind::Hook, "0.1.0", &options).unwrap();
    assert!(matches!(
        c.call("slow", json!({})),
        Err(crate::ClientError::Timeout)
    ));
    c.set_timeout(Duration::from_secs(3));
    assert_eq!(c.call("slow", json!({})).unwrap(), json!("late"));
}
