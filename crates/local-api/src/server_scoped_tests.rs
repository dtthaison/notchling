#![cfg(unix)]

use crate::client::{Client, ClientError, ConnectOptions};
use crate::server::{AgentIdentity, Caller, Handler, Limits, OpSpec, Server, ServerConfig};
use crate::testutil::TempDir;
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode};
use notchling_events::{Emit, MemorySink};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct Probe {
    hellos: AtomicUsize,
}

impl Handler for Probe {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == "read.none").then_some(OpSpec::AGENT_READ)
    }

    fn known_ops(&self) -> &[&'static str] {
        &["read.none"]
    }

    fn topics(&self) -> &[&str] {
        &["settings"]
    }

    fn identity_of(&self, _pid: u32) -> AgentIdentity {
        let n = self.hellos.fetch_add(1, Ordering::SeqCst);
        let mut who = AgentIdentity::shell();
        match n {
            0 | 1 => {
                who.verified = true;
                who.session = Some(format!("sess-{n}"));
            }
            2 => who.session = Some("sess-0".to_owned()),
            _ => {}
        }
        who
    }

    fn call(&self, _caller: &Caller, _op: &str, _args: &Value) -> Result<Value, ApiError> {
        Ok(json!({}))
    }
}

fn connect(socket: &std::path::Path) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(socket, ClientKind::Cli, "0.1.0", &options).unwrap()
}

fn serve(tag: &str) -> (TempDir, std::path::PathBuf, crate::server::ServerHandle) {
    serve_recorded(tag, None)
}

fn serve_recorded(
    tag: &str,
    sink: Option<Arc<MemorySink>>,
) -> (TempDir, std::path::PathBuf, crate::server::ServerHandle) {
    let dir = TempDir::new(tag);
    let socket = dir.path().join("run/notchlingd.sock");
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::empty(),
            events: sink.map(|s| s as Arc<dyn Emit>),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Probe::default()),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    (dir, socket, handle)
}

#[test]
fn a_scoped_event_reaches_only_the_verified_session_that_owns_it() {
    let (_dir, socket, handle) = serve("srv-scoped");
    let mut owner = connect(&socket);
    let mut other = connect(&socket);
    let mut unverified = connect(&socket);
    let mut shell = connect(&socket);
    owner.subscribe(&["mine.consents"]).unwrap();
    other.subscribe(&["mine.consents"]).unwrap();
    for client in [&mut unverified, &mut shell] {
        match client.subscribe(&["mine.consents"]) {
            Err(ClientError::Api(e)) => {
                assert_eq!(e.code, ErrorCode::Forbidden);
                assert_eq!(e.data.as_ref().unwrap()["reason"], "no_session");
            }
            other => panic!("{other:?}"),
        }
        client.subscribe(&["settings"]).unwrap();
    }
    assert_eq!(
        handle.publish_to(
            "sess-0",
            "mine.consents",
            "consent.changed",
            &json!({"a": 1})
        ),
        1
    );
    let got = owner.next_event(Duration::from_secs(2)).unwrap().unwrap();
    assert_eq!(got.event, "consent.changed");
    assert!(other
        .next_event(Duration::from_millis(200))
        .unwrap()
        .is_none());
    assert!(unverified
        .next_event(Duration::from_millis(100))
        .unwrap()
        .is_none());
    assert_eq!(
        handle.publish("mine.consents", "consent.changed", &json!({})),
        0
    );
    assert!(owner
        .next_event(Duration::from_millis(100))
        .unwrap()
        .is_none());
    assert_eq!(
        handle.publish("settings", "settings.changed", &json!({})),
        2
    );
    handle.shutdown();
}

#[test]
fn the_reserved_topics_say_which_phase_builds_them() {
    let (_dir, socket, handle) = serve("srv-reserved");
    let mut client = connect(&socket);
    for (topic, phase) in [("mine.jobs", 5), ("mine.sessions", 4)] {
        match client.subscribe(&[topic]) {
            Err(ClientError::Api(e)) => {
                assert_eq!(e.data.as_ref().unwrap()["reason"], "not_yet");
                assert_eq!(e.data.as_ref().unwrap()["phase"], phase);
            }
            other => panic!("{other:?}"),
        }
    }
    handle.shutdown();
}

#[test]
fn an_unknown_topic_is_a_bad_request_before_any_permission_check_and_never_a_refusal() {
    let sink = Arc::new(MemorySink::new());
    let (_dir, socket, handle) = serve_recorded("srv-unknown-topic", Some(sink.clone()));
    let _first = connect(&socket);
    let _second = connect(&socket);
    let mut unverified = connect(&socket);
    for topic in ["mine.bogus", "bogus"] {
        match unverified.subscribe(&[topic]) {
            Err(ClientError::Api(e)) => {
                assert_eq!(e.code, ErrorCode::BadRequest, "{topic}");
                assert_eq!(e.data.as_ref().unwrap()["reason"], "unknown_topic");
            }
            other => panic!("{topic}: {other:?}"),
        }
    }
    match unverified.subscribe(&["mine.consents", "mine.bogus"]) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::BadRequest),
        other => panic!("{other:?}"),
    }
    assert!(
        crate::testutil::summary(&sink)
            .iter()
            .all(|(name, _)| name != "api.refused"),
        "{:?}",
        crate::testutil::summary(&sink)
    );
    match unverified.subscribe(&["mine.consents"]) {
        Err(ClientError::Api(e)) => {
            assert_eq!(e.code, ErrorCode::Forbidden);
            assert_eq!(e.data.as_ref().unwrap()["reason"], "no_session");
        }
        other => panic!("{other:?}"),
    }
    assert!(crate::testutil::summary(&sink)
        .iter()
        .any(|(name, _)| name == "api.refused"));
    handle.shutdown();
}
