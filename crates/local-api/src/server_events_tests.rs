#![cfg(unix)]

use crate::client::{Client, ClientError, ConnectOptions};
use crate::hub::Hub;
use crate::presence::Presence;
use crate::server::{
    Caller, Handler, Limits, OpHome, OpSpec, Server, ServerConfig, ServerHandle, Shared,
};
use crate::testutil::{field_text, summary, TempDir};
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode};
use notchling_events::kinds::security::PeerWhy;
use notchling_events::{scope, MemorySink};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Probe {
    session_lookups: AtomicUsize,
}

impl Handler for Probe {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "app.do" | "app.kind" | "app.marks" | "app.refuses" => Some(OpSpec::APP),
            "app.quiet" => Some(OpSpec::APP_QUIET),
            "agent.who" => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    fn home(&self, op: &str) -> OpHome {
        match op {
            "app.kind" => OpHome::Kind,
            "app.quiet" => OpHome::Quiet,
            _ => OpHome::Called,
        }
    }

    fn known_ops(&self) -> &[&'static str] {
        &[
            "app.do",
            "app.kind",
            "app.marks",
            "app.refuses",
            "app.quiet",
            "agent.who",
        ]
    }

    fn session_of(&self, _pid: u32) -> Option<String> {
        self.session_lookups.fetch_add(1, Ordering::SeqCst);
        Some("sess-1".to_owned())
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            "app.marks" => {
                scope::mark_recorded();
                Ok(json!("done"))
            }
            "app.refuses" => Err(ApiError::new(ErrorCode::Forbidden, "switch is off")),
            "agent.who" => {
                let current = scope::current();
                let actor = current.actor.expect("a request runs in a scope");
                Ok(json!({
                    "tier": format!("{:?}", actor.tier),
                    "via": format!("{:?}", actor.via),
                    "session": actor.session.map(|s| s.as_str().to_owned()),
                    "has_exe": actor.exe.is_some(),
                    "has_op_id": current.op_id.is_some(),
                }))
            }
            _ => Ok(json!("done")),
        }
    }
}

struct Running {
    _dir: TempDir,
    socket: PathBuf,
    sink: Arc<MemorySink>,
    probe: Arc<Probe>,
    handle: ServerHandle,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

fn start(allow: Allowlist) -> Running {
    let dir = TempDir::new("srv-ev");
    let socket = dir.path().join("run/notchlingd.sock");
    let sink = Arc::new(MemorySink::new());
    let probe = Arc::new(Probe::default());
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: allow,
            events: Some(sink.clone()),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        probe.clone(),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    Running {
        _dir: dir,
        socket,
        sink,
        probe,
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

fn app_run() -> Running {
    start(Allowlist::new([std::env::current_exe().unwrap()]))
}

fn without_presence(run: &Running) -> Vec<(String, String)> {
    summary(&run.sink)
        .into_iter()
        .filter(|(name, _)| !name.starts_with("app."))
        .collect()
}

#[test]
fn forbidden_op_records_api_refused_with_exe() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run, ClientKind::Cli);
    match agent.call("app.do", json!({})) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    let events = run.sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].spec.name, "api.refused");
    assert_eq!(field_text(&events[0], "op").as_deref(), Some("app.do"));
    assert_eq!(field_text(&events[0], "tier").as_deref(), Some("agent"));
    assert_eq!(field_text(&events[0], "why").as_deref(), Some("forbidden"));
    assert!(events[0].fields.get("exe").is_some());
}

#[test]
fn a_handler_refusal_is_an_api_refused_not_a_failure() {
    let run = app_run();
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("app.refuses", json!({})).is_err());
    assert_eq!(
        without_presence(&run),
        vec![("api.refused".to_owned(), "app.refuses".to_owned())]
    );
    let shown = format!("{:?}", run.sink.events().last().unwrap().fields);
    assert!(!shown.contains("switch is off"), "{shown}");
}

#[test]
fn unknown_peer_records_api_unknown_peer() {
    let sink = Arc::new(MemorySink::new());
    let shared = Shared {
        handler: Arc::new(Probe::default()),
        allowlist: Allowlist::empty(),
        events: Some(sink.clone()),
        presence: Presence::new(),
        version: "1".into(),
        limits: Limits::default(),
        hub: Hub::default(),
        started: Instant::now(),
        stop: AtomicBool::new(false),
        my_uid: 0,
        open: AtomicUsize::new(0),
    };
    shared.unknown_peer(PeerWhy::OtherUser, Some(std::path::Path::new("/bin/other")));
    shared.unknown_peer(PeerWhy::PeerUnknown, None);
    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].spec.name, "api.unknown_peer");
    assert_eq!(field_text(&events[0], "why").as_deref(), Some("other_user"));
    assert_eq!(field_text(&events[0], "exe").as_deref(), Some("other"));
    assert_eq!(
        field_text(&events[1], "why").as_deref(),
        Some("peer_unknown")
    );
    assert!(events[1]
        .fields
        .get("exe")
        .is_none_or(|v| { *v == notchling_events::field::FieldValue::Null }));
}

#[test]
fn app_success_records_op_called_for_called_home() {
    let run = app_run();
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("app.do", json!({})).is_ok());
    assert_eq!(
        without_presence(&run),
        vec![("op.called".to_owned(), "app.do".to_owned())]
    );
    let event = run
        .sink
        .events()
        .into_iter()
        .find(|e| e.spec.name == "op.called")
        .unwrap();
    assert_eq!(field_text(&event, "tier").as_deref(), Some("app"));
    assert_eq!(field_text(&event, "result").as_deref(), Some("ok"));
}

#[test]
fn quiet_success_records_nothing() {
    let run = app_run();
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("app.quiet", json!({})).is_ok());
    assert!(app.call("app.kind", json!({})).is_ok());
    assert_eq!(without_presence(&run), Vec::new());
}

#[test]
fn area_marked_recorded_suppresses_generic_event() {
    let run = app_run();
    let mut app = connect(&run, ClientKind::App);
    assert!(app.call("app.marks", json!({})).is_ok());
    assert!(app.call("app.do", json!({})).is_ok());
    assert_eq!(
        without_presence(&run),
        vec![("op.called".to_owned(), "app.do".to_owned())]
    );
}

#[test]
fn agent_session_is_resolved_once_at_hello() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run, ClientKind::Hook);
    for _ in 0..3 {
        let who = agent.call("agent.who", json!({})).unwrap();
        assert_eq!(who["session"], "sess-1");
        assert_eq!(who["via"], "Some(Hook)");
        assert_eq!(who["tier"], "Agent");
        assert_eq!(who["has_exe"], true);
        assert_eq!(who["has_op_id"], true);
    }
    assert_eq!(run.probe.session_lookups.load(Ordering::SeqCst), 1);
}

#[test]
fn the_app_attaches_once_and_detaches_after_it_drops() {
    let run = app_run();
    let first = connect(&run, ClientKind::App);
    let second = connect(&run, ClientKind::App);
    let attached = || {
        run.sink
            .events()
            .iter()
            .filter(|e| e.spec.name == "app.attached")
            .count()
    };
    assert_eq!(attached(), 1);
    drop(first);
    drop(second);
    std::thread::sleep(Duration::from_millis(300));
    assert!(run.sink.names().iter().all(|n| *n != "app.detached"));
    assert_eq!(attached(), 1);
}
