#![cfg(unix)]

use crate::client::{Client, ConnectOptions};
use crate::server::{
    AgentCall, AgentIdentity, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle,
};
use crate::testutil::{field_text, TempDir};
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode};
use notchling_events::field::FieldValue;
use notchling_events::kinds::choices::AgentTool;
use notchling_events::scope::{self, ConsentNote};
use notchling_events::{Event, MemorySink};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Probe {
    named: Mutex<Vec<AgentCall>>,
}

impl Handler for Probe {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "act.ok" | "act.fail" | "act.refuse" | "act.pending" | "act.fake" | "act.own"
            | "act.used" | "act.panic" | "act.panic_noted" | "act.panic_marked" => {
                Some(OpSpec::AGENT_ACTS)
            }
            "read.ok" | "read.fail" => Some(OpSpec::AGENT_READ),
            "hook.x" => Some(OpSpec::AGENT_HOOK),
            "app.act" => Some(OpSpec::APP),
            "gated.act" => Some(OpSpec::APPROVAL),
            _ => None,
        }
    }

    fn known_ops(&self) -> &[&'static str] {
        &[
            "act.ok",
            "act.fail",
            "act.refuse",
            "act.pending",
            "act.fake",
            "act.own",
            "act.used",
            "act.panic",
            "act.panic_noted",
            "act.panic_marked",
            "read.ok",
            "read.fail",
            "hook.x",
            "app.act",
            "gated.act",
        ]
    }

    fn identity_of(&self, _pid: u32) -> AgentIdentity {
        let mut who = AgentIdentity::shell();
        who.tool = AgentTool::Claude;
        who.verified = true;
        who.session = Some("sess-9".to_owned());
        who.organization = Some("org-1".to_owned());
        who.cmd_hash = Some("aabbccdd11223344".to_owned());
        who
    }

    fn agent_called(&self, call: &AgentCall) {
        self.named.lock().unwrap().push(call.clone());
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            "act.fail" | "read.fail" => Err(ApiError::new(ErrorCode::Failed, "broke")),
            "act.panic" => panic!("a handler bug"),
            "act.panic_noted" => {
                scope::note_consent("req-5", ConsentNote::Pending);
                panic!("a handler bug")
            }
            "act.panic_marked" => {
                scope::mark_recorded();
                panic!("a handler bug")
            }
            "act.refuse" => Err(ApiError::new(ErrorCode::Forbidden, "off")),
            "act.pending" => {
                scope::note_consent("req-1", ConsentNote::Pending);
                Ok(json!({"id": "req-1", "state": "waiting"}))
            }
            "act.used" => {
                scope::note_consent("req-2", ConsentNote::Used);
                Ok(json!({"applied": true}))
            }
            "act.fake" => Ok(json!({"id": "req-3", "state": "waiting"})),
            "act.own" => {
                scope::mark_recorded();
                Ok(json!("done"))
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
    let dir = TempDir::new("srv-agent");
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

fn connect(run: &Running) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(&run.socket, ClientKind::Cli, "0.1.0", &options).unwrap()
}

fn agent_calls(run: &Running) -> Vec<Event> {
    run.sink
        .events()
        .into_iter()
        .filter(|e| e.spec.name == "agent.called")
        .collect()
}

fn has(event: &Event, name: &str) -> bool {
    !matches!(event.fields.get(name), None | Some(FieldValue::Null))
}

fn results(run: &Running) -> Vec<String> {
    agent_calls(run)
        .iter()
        .map(|e| field_text(e, "result").unwrap_or_default())
        .collect()
}

#[test]
fn an_acting_call_records_one_line_whether_it_works_is_refused_or_fails() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.ok", json!({})).is_ok());
    assert!(agent.call("act.refuse", json!({})).is_err());
    assert!(agent.call("act.fail", json!({})).is_err());
    assert_eq!(results(&run), ["ok", "refused", "failed"]);
    let calls = agent_calls(&run);
    assert_eq!(field_text(&calls[0], "op").as_deref(), Some("act.ok"));
    assert_eq!(field_text(&calls[0], "tool").as_deref(), Some("claude"));
    assert_eq!(field_text(&calls[2], "code").as_deref(), Some("failed"));
}

#[test]
fn an_acting_call_whose_handler_panics_is_still_recorded_as_failed() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.panic", json!({})).is_err());
    assert_eq!(results(&run), ["failed"]);
    let calls = agent_calls(&run);
    assert_eq!(field_text(&calls[0], "op").as_deref(), Some("act.panic"));
    assert_eq!(field_text(&calls[0], "code").as_deref(), Some("failed"));
    assert_eq!(run.probe.named.lock().unwrap().len(), 1);
}

#[test]
fn a_panic_after_the_handler_noted_a_request_or_marked_its_own_kind_is_still_one_failed_line() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.panic_noted", json!({})).is_err());
    let mut again = connect(&run);
    assert!(again.call("act.panic_marked", json!({})).is_err());
    assert_eq!(results(&run), ["failed", "failed"]);
    assert!(agent_calls(&run).iter().all(|line| !has(line, "consent")));
}

#[test]
fn a_read_a_hook_op_and_an_app_op_record_no_agent_line() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("read.ok", json!({})).is_ok());
    assert!(agent.call("hook.x", json!({})).is_ok());
    assert!(agent.call("app.act", json!({})).is_err());
    assert!(agent.call("read.fail", json!({})).is_err());
    assert!(agent_calls(&run).is_empty());
    let app = start(Allowlist::new([std::env::current_exe().unwrap()]));
    let mut client = connect(&app);
    assert!(client.call("app.act", json!({})).is_ok());
    assert!(agent_calls(&app).is_empty());
}

#[test]
fn a_handler_that_recorded_its_own_kind_still_gets_the_agent_line() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.own", json!({})).is_ok());
    assert_eq!(results(&run), ["ok"]);
}

#[test]
fn pending_comes_from_the_scope_note_never_from_the_reply() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.pending", json!({})).is_ok());
    assert!(agent.call("act.fake", json!({})).is_ok());
    assert!(agent.call("act.used", json!({})).is_ok());
    assert_eq!(results(&run), ["pending", "ok", "ok"]);
    let calls = agent_calls(&run);
    assert!(has(&calls[0], "consent"));
    assert!(!has(&calls[1], "consent"));
    assert!(has(&calls[2], "consent"));
}

#[test]
fn a_dry_run_is_recorded_as_a_dry_run() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.ok", json!({"dryRun": true})).is_ok());
    assert!(agent.call("act.fail", json!({"dryRun": true})).is_err());
    assert_eq!(results(&run), ["dry_run", "failed"]);
}

#[test]
fn the_line_carries_the_session_the_executable_and_the_identity() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("act.ok", json!({})).is_ok());
    let line = agent_calls(&run).remove(0);
    assert!(has(&line, "verified"));
    assert!(has(&line, "organization"));
    assert!(has(&line, "cmd_hash"));
    assert!(!has(&line, "out_hash"));
    let named = run.probe.named.lock().unwrap();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].identity.session.as_deref(), Some("sess-9"));
    let actor = named[0].scope.actor.clone().expect("an actor");
    assert_eq!(
        actor.session.map(|s| s.as_str().to_owned()),
        Some("sess-9".to_owned())
    );
    assert!(actor.exe.is_some());
    assert!(named[0].scope.op_id.is_some());
}

#[test]
fn an_approval_op_with_no_app_is_refused_and_recorded() {
    let run = start(Allowlist::empty());
    let mut agent = connect(&run);
    assert!(agent.call("gated.act", json!({})).is_err());
    assert_eq!(results(&run), ["refused"]);
    assert_eq!(
        field_text(&agent_calls(&run)[0], "code").as_deref(),
        Some("needs_approval")
    );
}
