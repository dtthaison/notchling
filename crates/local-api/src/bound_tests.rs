#![cfg(unix)]

use crate::bound_identity;
use crate::client::{Client, ConnectOptions};
use crate::server::{
    AgentIdentity, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle,
};
use crate::testutil::TempDir;
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct Probe {
    looked_up: AtomicUsize,
}

impl Handler for Probe {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == "read.who").then_some(OpSpec::AGENT_READ)
    }

    fn known_ops(&self) -> &[&'static str] {
        &["read.who"]
    }

    fn identity_of(&self, _pid: u32) -> AgentIdentity {
        let n = self.looked_up.fetch_add(1, Ordering::SeqCst) + 1;
        let mut who = AgentIdentity::shell();
        who.verified = true;
        who.session = Some(format!("sess-{n}"));
        who
    }

    fn call(&self, _caller: &Caller, _op: &str, _args: &Value) -> Result<Value, ApiError> {
        Ok(json!({"session": bound_identity().and_then(|w| w.session)}))
    }
}

struct Running {
    _dir: TempDir,
    socket: std::path::PathBuf,
    handle: ServerHandle,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

fn start() -> Running {
    let dir = TempDir::new("srv-bound");
    let socket = dir.path().join("run/notchlingd.sock");
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::empty(),
            events: None,
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Probe::default()),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    Running {
        _dir: dir,
        socket,
        handle,
    }
}

fn connect(run: &Running) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(&run.socket, ClientKind::Cli, "0.1.0", &options).unwrap()
}

#[test]
fn every_call_of_a_connection_sees_the_identity_worked_out_at_hello() {
    let run = start();
    let mut first = connect(&run);
    let mut second = connect(&run);
    for _ in 0..3 {
        assert_eq!(
            first.call("read.who", json!({})).unwrap()["session"],
            "sess-1"
        );
    }
    assert_eq!(
        second.call("read.who", json!({})).unwrap()["session"],
        "sess-2"
    );
}

#[test]
fn nothing_is_bound_outside_a_call() {
    assert!(bound_identity().is_none());
}
