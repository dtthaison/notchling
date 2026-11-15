use super::{route, Forward, NodeForward, PromptFollowers};
use crate::acting_ops::Room;
use crate::acting_ops_tests::{app, caller, sessions, setup, PROMPT};
use crate::cluster_testkit::{eventually, pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

fn me() -> NodeId {
    NodeId::parse("0123456789abcdef").unwrap()
}

fn other() -> NodeId {
    NodeId::parse("fedcba9876543210").unwrap()
}

fn client_home() -> Opts {
    let mut opts = Opts::named("home");
    opts.client = true;
    opts
}

fn caller_of(tier: Tier, client: ClientKind) -> Caller {
    Caller {
        tier,
        client,
        peer: None,
        pid: None,
    }
}

#[test]
fn route_names_the_owner_and_hands_it_the_bare_id() {
    let args = json!({"id": format!("{}/abc", other()), "text": "hi"});
    assert_eq!(
        route("session.reply", &args, &me()),
        Some((other(), json!({"id": "abc", "text": "hi"})))
    );
    let agy = json!({"id": format!("{}/agy:7", other())});
    assert_eq!(
        route("agy.conversation", &agy, &me()),
        Some((other(), json!({"id": "agy:7"})))
    );
}

#[test]
fn bare_id_stays_local() {
    let me = me();
    assert_eq!(route("session.reply", &json!({"id": "abc"}), &me), None);
    assert_eq!(route("session.reply", &json!({"id": "a/b"}), &me), None);
    assert_eq!(route("session.reply", &json!({"id": 7}), &me), None);
    assert_eq!(route("session.reply", &json!({}), &me), None);
    assert_eq!(route("session.reply", &Value::Null, &me), None);
}

#[test]
fn own_qualified_id_stays_local() {
    let args = json!({"id": format!("{}/abc", me())});
    assert_eq!(route("session.reply", &args, &me()), None);
}

#[test]
fn an_op_with_no_id_rule_never_routes() {
    let args = json!({"id": format!("{}/abc", other())});
    assert_eq!(route("session.new", &args, &me()), None);
    assert_eq!(route("settings.get", &args, &me()), None);
    assert_eq!(route("sessions.list", &args, &me()), None);
}

#[test]
fn qualified_reply_is_forwarded_to_owner() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(a.node.clone());
    let args = json!({"id": format!("{}/abc", b.id()), "text": "hi"});
    let out = forward
        .forward(&app(), "session.reply", &args)
        .expect("routed")
        .unwrap();
    assert_eq!(out, json!({"session": "s-session.reply"}));
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "session.reply");
    assert_eq!(calls[0].1, json!({"id": "abc", "text": "hi"}));
    assert_eq!(
        calls[0].2,
        Origin {
            node: a.id(),
            tier: OriginTier::App
        }
    );
}

#[test]
fn a_bare_id_is_not_forwarded() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(a.node.clone());
    let own = json!({"id": format!("{}/abc", a.id())});
    assert!(forward.forward(&app(), "session.reply", &own).is_none());
    assert!(forward
        .forward(&app(), "session.reply", &json!({"id": "abc"}))
        .is_none());
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn only_the_app_forwards_and_an_agent_is_told_so_without_a_round_trip() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(a.node.clone());
    let args = json!({"id": format!("{}/abc", b.id()), "text": "hi"});
    let agent = caller_of(Tier::Agent, ClientKind::Cli);
    let err = forward
        .forward(&agent, "session.reply", &args)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn a_call_that_came_from_a_node_never_forwards_again() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(a.node.clone());
    let args = json!({"id": format!("{}/abc", b.id()), "text": "hi"});
    let relayed = caller_of(Tier::App, ClientKind::Node);
    assert!(forward.forward(&relayed, "session.reply", &args).is_none());
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn an_unknown_node_is_an_error_and_never_a_fallback() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(a.node.clone());
    let args = json!({"id": format!("{}/abc", other()), "text": "hi"});
    let err = forward
        .forward(&app(), "session.reply", &args)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(a.calls.calls.lock().unwrap().is_empty());
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn a_node_in_no_cluster_forwards_nothing() {
    let net = MemNet::new();
    let alone = crate::cluster_testkit::node(&net, Opts::named("alone"));
    let forward = NodeForward::new(alone.node.clone());
    let args = json!({"id": format!("{}/abc", other())});
    let err = forward
        .forward(&app(), "session.reply", &args)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(alone.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn forward_from_worker_is_refused() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(b.node.clone());
    let args = json!({"id": format!("{}/abc", a.id()), "text": "hi"});
    let err = forward
        .forward(&app(), "session.reply", &args)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    assert!(a.calls.calls.lock().unwrap().is_empty());
    eventually("a cluster.refused event", || {
        !a.room.named("cluster.refused").is_empty()
    });
    eventually("an audit line", || a.audit_text().contains("step=refused"));
}

#[test]
fn the_lease_on_prompts_runs_out() {
    let followers = PromptFollowers::default();
    assert!(!followers.active(0));
    followers.follow("n1", 1_000);
    assert!(followers.active(999));
    assert!(!followers.active(1_000));
    followers.follow("n1", 5_000);
    followers.follow("n1", 2_000);
    assert!(followers.active(4_999), "a shorter renewal never shortens");
    followers.follow("n2", 9_000);
    assert!(followers.active(8_000));
    assert!(!followers.active(9_000));
}

fn permission() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hook/permission-request.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn hook() -> Caller {
    caller(ClientKind::Hook)
}

#[test]
fn headless_node_waits_for_cluster_follower_on_permission() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    s.room.app.store(false, Ordering::SeqCst);
    let ask = json!({"event": permission(), "pid": std::process::id(), "waitMs": 10_000});
    let out = s.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(
        out,
        json!({"action": "pass", "reason": "no app is attached"}),
        "nobody follows: refused at once"
    );
    assert!(s.room.named("prompt.pending").is_empty());

    s.room.follow_prompts("0123456789abcdef", 60_000);
    let asking = s.clone();
    let thread =
        std::thread::spawn(move || asking.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let id = loop {
        if let Some(p) = s.room.named("prompt.pending").first() {
            break p["id"].as_str().unwrap().to_string();
        }
        assert!(std::time::Instant::now() < deadline, "never held");
        std::thread::sleep(Duration::from_millis(10));
    };
    s.ops
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "allow"}),
        )
        .unwrap();
    assert_eq!(thread.join().unwrap()["action"], "allow");
}

#[test]
fn the_vault_ops_go_only_to_the_node_that_holds_the_vault() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let own = NodeForward::new(a.node.clone());
    let there = json!({"node": a.id()});
    assert!(own.forward(&app(), "vault.list", &there).is_none());
    assert!(own.forward(&app(), "vault.list", &json!({})).is_none());
    let elsewhere = json!({"node": b.id()});
    let err = own
        .forward(&app(), "vault.list", &elsewhere)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    let agent = caller_of(Tier::Agent, ClientKind::Cli);
    let err = own
        .forward(&agent, "vault.save", &there)
        .expect("routed")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(a.calls.calls.lock().unwrap().is_empty());
    assert!(b.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn a_vault_op_that_names_a_node_never_falls_back_to_this_machines_vault() {
    let net = MemNet::new();
    let alone = crate::cluster_testkit::node(&net, Opts::named("alone"));
    let forward = NodeForward::new(alone.node.clone());
    let named = json!({"node": other()});
    for op in ["vault.list", "vault.save", "vault.remove"] {
        let err = forward
            .forward(&app(), op, &named)
            .expect("routed")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Failed, "{op}");
        let err = forward
            .forward(&app(), op, &json!({"node": 7}))
            .expect("routed")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{op}");
        assert!(forward.forward(&app(), op, &json!({})).is_none(), "{op}");
        assert!(forward
            .forward(&app(), op, &json!({"node": null}))
            .is_none());
    }
    assert!(alone.calls.calls.lock().unwrap().is_empty());
}

#[test]
fn a_worker_cannot_use_the_vault_through_the_node_link() {
    let net = MemNet::new();
    let (a, b) = pair(&net, client_home(), Opts::named("worker"));
    let forward = NodeForward::new(b.node.clone());
    let got = forward
        .forward(&app(), "vault.list", &json!({"node": a.id()}))
        .expect("routed");
    assert!(got.is_err(), "{got:?}");
    assert!(a.calls.calls.lock().unwrap().is_empty());
}
