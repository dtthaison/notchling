use super::{checked, loopback_ok, node_caller, DaemonCalls};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::LocalCalls;
use crate::cluster_switches::LateHandler;
use crate::cluster_testkit::Dir;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ApiError, Caller, ClientKind, ErrorCode, Handler, OpSpec, Tier};
use serde_json::{json, Value};
use std::ffi::OsStr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<(String, Tier, ClientKind)>>,
}

impl Handler for Recorder {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "x.agent" => Some(OpSpec::AGENT),
            "x.app" => Some(OpSpec::APP),
            "x.approval" => Some(OpSpec::APPROVAL),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push((op.to_string(), caller.tier, caller.client));
        Ok(json!({"ran": op}))
    }
}

fn origin(tier: OriginTier) -> Origin {
    Origin {
        node: NodeId::parse("0123456789abcdef").unwrap(),
        tier,
    }
}

#[test]
fn loopback_needs_the_switch_and_a_dev_build() {
    let dir = Dir::new("loopback");
    let exe = dir.path().join("notchling");
    std::fs::write(&exe, b"").unwrap();
    let built = dir.path().join("target").join("debug");
    std::fs::create_dir_all(&built).unwrap();
    let dev = built.join("notchling");
    std::fs::write(&dev, b"").unwrap();
    assert!(!loopback_ok(Some(OsStr::new("1")), Some(&exe)));
    assert!(!loopback_ok(Some(OsStr::new("0")), Some(&dev)));
    assert!(!loopback_ok(None, Some(&dev)));
    assert!(!loopback_ok(Some(OsStr::new("1")), None));
    assert!(loopback_ok(Some(OsStr::new("1")), Some(&dev)));
}

#[test]
fn only_an_app_origin_runs_as_the_app() {
    assert_eq!(node_caller(&origin(OriginTier::App)).tier, Tier::App);
    assert_eq!(node_caller(&origin(OriginTier::Agent)).tier, Tier::Agent);
    assert_eq!(node_caller(&origin(OriginTier::Node)).tier, Tier::Agent);
    let caller = node_caller(&origin(OriginTier::App));
    assert_eq!(caller.client, ClientKind::Node);
    assert_eq!(caller.peer, None);
    assert_eq!(caller.pid, None);
}

#[test]
fn a_node_call_meets_the_socket_checks() {
    let handler = Recorder::default();
    let room = FakeRoom::default();
    let agent = node_caller(&origin(OriginTier::Node));
    let app = node_caller(&origin(OriginTier::App));
    let code = |r: Result<Value, ApiError>| r.unwrap_err().code;
    assert_eq!(
        code(checked(&handler, &room, &app, "x.none", &json!({}))),
        ErrorCode::UnknownOp
    );
    assert_eq!(
        code(checked(&handler, &room, &agent, "x.app", &json!({}))),
        ErrorCode::Forbidden
    );
    assert_eq!(
        code(checked(&handler, &room, &agent, "x.approval", &json!({}))),
        ErrorCode::NeedsApproval
    );
    assert!(handler.calls.lock().unwrap().is_empty());
    room.app.store(true, Ordering::SeqCst);
    checked(&handler, &room, &agent, "x.approval", &json!({})).unwrap();
    checked(&handler, &room, &app, "x.app", &json!({})).unwrap();
    checked(&handler, &room, &agent, "x.agent", &json!({})).unwrap();
    let calls = handler.calls.lock().unwrap();
    assert_eq!(
        *calls,
        vec![
            ("x.approval".to_string(), Tier::Agent, ClientKind::Node),
            ("x.app".to_string(), Tier::App, ClientKind::Node),
            ("x.agent".to_string(), Tier::Agent, ClientKind::Node),
        ]
    );
}

#[test]
fn daemon_calls_wait_for_the_server_and_never_keep_it_alive() {
    let late = Arc::new(LateHandler::default());
    let calls = DaemonCalls::new(late.clone(), Arc::new(FakeRoom::default()));
    let o = origin(OriginTier::Node);
    let err = calls.call("x.agent", &json!({}), &o).unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    let handler: Arc<dyn Handler> = Arc::new(Recorder::default());
    late.serve(&handler);
    assert_eq!(
        calls.call("x.agent", &json!({}), &o).unwrap(),
        json!({"ran": "x.agent"})
    );
    drop(handler);
    assert_eq!(
        calls.call("x.agent", &json!({}), &o).unwrap_err().code,
        ErrorCode::Failed
    );
}
