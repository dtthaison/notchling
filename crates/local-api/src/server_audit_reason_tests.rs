#![cfg(unix)]

use crate::client::{Client, ConnectOptions};
use crate::server::{Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle};
use crate::testutil::{summary, TempDir};
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode, Tier};
use notchling_events::MemorySink;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

const REFUSAL: &str = "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal.";

struct Refusing;

impl Handler for Refusing {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        matches!(op, "session.permission" | "accounts.switch").then_some(OpSpec::APP)
    }

    fn known_ops(&self) -> &[&'static str] {
        &["session.permission", "accounts.switch"]
    }

    fn topics(&self) -> &[&str] {
        &[]
    }

    fn topic_tier(&self, _topic: &str) -> Tier {
        Tier::Agent
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if args.get("fail") == Some(&json!(false)) {
            return Ok(json!({"decided": "allow"}));
        }
        match op {
            "session.permission" => Err(ApiError::new(ErrorCode::Failed, REFUSAL)),
            _ => Err(ApiError::new(
                ErrorCode::Failed,
                "no login for me@example.com in /Users/me/.claude",
            )),
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

fn start() -> Running {
    let dir = TempDir::new("srv-reason");
    let socket = dir.path().join("run/notchlingd.sock");
    let sink = Arc::new(MemorySink::new());
    let me = std::env::current_exe().unwrap();
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::new([me]),
            events: Some(sink.clone()),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Refusing),
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

fn app(run: &Running) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(&run.socket, ClientKind::App, "0.1.0", &options).unwrap()
}

#[test]
fn a_failed_answer_is_recorded_by_its_code_and_never_by_its_text() {
    let run = start();
    let mut client = app(&run);
    assert!(client.call("session.permission", json!({})).is_err());
    assert!(client
        .call("session.permission", json!({"fail": false}))
        .is_ok());
    let names: Vec<(String, String)> = summary(&run.sink)
        .into_iter()
        .filter(|(name, _)| name != "app.attached")
        .collect();
    assert_eq!(
        names,
        vec![
            ("op.failed".to_owned(), "session.permission".to_owned()),
            ("op.called".to_owned(), "session.permission".to_owned()),
        ]
    );
    let shown = format!(
        "{:?}",
        run.sink
            .events()
            .iter()
            .map(|e| &e.fields)
            .collect::<Vec<_>>()
    );
    assert!(!shown.contains("terminal"), "{shown}");
    assert!(!shown.contains("nothing was typed"), "{shown}");
}

#[test]
fn another_ops_error_text_stays_out_of_the_log() {
    let run = start();
    let mut client = app(&run);
    assert!(client.call("accounts.switch", json!({})).is_err());
    let shown = format!(
        "{:?}",
        run.sink
            .events()
            .iter()
            .map(|e| &e.fields)
            .collect::<Vec<_>>()
    );
    assert!(!shown.contains("example.com"), "{shown}");
    assert!(!shown.contains("/Users/me"), "{shown}");
    assert!(shown.contains("accounts.switch"), "{shown}");
}
