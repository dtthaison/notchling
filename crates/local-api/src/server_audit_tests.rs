//! Milestone 3's additions to the server: every app-tier call is audited with its outcome, the
//! daemon can ask whether the app is attached, and a client can lengthen its wait for one call.
#![cfg(unix)]

use crate::client::{Client, ConnectOptions};
use crate::server::{Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle};
use crate::testutil::TempDir;
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode};
use crate::AuditLog;
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
            _ => None,
        }
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            "session.stop" => Err(ApiError::new(ErrorCode::Unimplemented, "not yet")),
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
    audit: PathBuf,
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
    let audit = dir.path().join("audit.jsonl");
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: allow,
            audit: Some(AuditLog::new(audit.clone())),
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
        audit,
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

fn audit_lines(run: &Running) -> Vec<Value> {
    std::fs::read_to_string(&run.audit)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn app_tier_calls_are_audited_with_their_outcome() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    let mut app = connect(&run, ClientKind::App);
    assert_eq!(
        app.call("session.reply", json!({})).unwrap(),
        json!({"sent": true})
    );
    assert!(app.call("session.stop", json!({})).is_err());
    assert_eq!(app.call("read", json!({})).unwrap(), json!({"sent": true}));
    let lines = audit_lines(&run);
    let summary: Vec<(String, String, String)> = lines
        .iter()
        .map(|l| {
            (
                l["op"].as_str().unwrap().to_string(),
                l["tier"].as_str().unwrap().to_string(),
                l["result"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("session.reply".into(), "app".into(), "ok".into()),
            ("session.stop".into(), "app".into(), "unimplemented".into()),
        ],
        "agent-tier reads aren't audited"
    );
}

#[test]
fn the_daemon_knows_when_the_app_is_attached() {
    let me = std::env::current_exe().unwrap();
    let run = start(Allowlist::new([me]));
    assert!(!run.handle.app_attached());
    // The app's own binary saying hello as the hook or the command line is no one to answer.
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
    // A client the allowlist doesn't name is never the app, whatever it says it is.
    let agent_run = start(Allowlist::empty());
    let _posing = connect(&agent_run, ClientKind::App);
    assert!(!agent_run.handle.app_attached());
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
