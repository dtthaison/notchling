use crate::acting_ops_tests::{app, caller, FakeRoom};
use crate::cluster_forward::{Forward, NodeForward};
use crate::cluster_node::LocalCalls;
use crate::cluster_switches::LateHandler;
use crate::cluster_testkit::{pair, Opts, TestNode};
use crate::cluster_wiring::DaemonCalls;
use crate::daemon::Ops;
use crate::install_ops_tests::{data, ops_for, rig, settings_text, Dir, USER_SETTINGS};
use crate::switches::INSTALL_OFF;
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::LocalCmd;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Handler, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct Node {
    _handler: Arc<dyn Handler>,
    calls: Arc<dyn LocalCalls>,
    dir: Dir,
}

fn node_with_install(tag: &str, on: bool) -> Node {
    let dir = Dir::new(tag);
    let (install, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), on);
    let handler: Arc<dyn Handler> =
        Arc::new(Ops::new(Arc::new(Mutex::new(json!([])))).with_group(Arc::new(install)));
    let late = Arc::new(LateHandler::default());
    late.serve(&handler);
    let calls: Arc<dyn LocalCalls> = DaemonCalls::new(late, Arc::new(FakeRoom::default()));
    Node {
        _handler: handler,
        calls,
        dir,
    }
}

fn cluster(node: &Node) -> (TestNode, TestNode) {
    let net = MemNet::new();
    let mut worker = Opts::named("worker");
    worker.headless = true;
    worker.local = Some(node.calls.clone());
    pair(&net, Opts::named("home"), worker)
}

fn hook_entries(node: &Node) -> usize {
    let settings = data(&settings_text(&node.dir));
    ["PreToolUse", "PermissionRequest", "PostToolUse"]
        .iter()
        .flat_map(|event| settings["hooks"][event].as_array().into_iter().flatten())
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .filter(|h| {
            h["command"]
                .as_str()
                .is_some_and(|c| c.contains("notchling"))
        })
        .count()
}

fn forward_from(home: &TestNode, op: &str, to: &TestNode) -> Result<Value, ApiError> {
    NodeForward::new(home.node.clone())
        .forward(&app(), op, &json!({"node": to.id()}))
        .expect("routed to the node")
}

#[test]
fn forwarded_apply_installs_on_node_with_install_on() {
    let target = node_with_install("fwd-on", true);
    let (home, worker) = cluster(&target);
    let reply = forward_from(&home, ops::INSTALL_APPLY, &worker).unwrap();
    assert_eq!(reply["applied"], true, "{reply}");
    assert_eq!(reply["on"], true);
    assert!(hook_entries(&target) > 0);
    let installed = data(&settings_text(&target.dir));
    assert_eq!(installed["model"], "opus");
    assert!(installed["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["matcher"] == "*"
            && g["hooks"][0]["command"]
                .as_str()
                .is_some_and(|c| c.ends_with("hook --fence"))));
    let again = forward_from(&home, ops::INSTALL_PLAN, &worker).unwrap();
    assert_eq!(again["plan"]["changed"], false);
}

#[test]
fn install_off_refuses_forwarded_apply() {
    let target = node_with_install("fwd-off", false);
    let (home, worker) = cluster(&target);
    let reply = forward_from(&home, ops::INSTALL_APPLY, &worker).unwrap();
    assert_eq!(reply["applied"], false, "{reply}");
    assert_eq!(reply["on"], false);
    assert_eq!(reply["message"], INSTALL_OFF);
    assert_eq!(settings_text(&target.dir), USER_SETTINGS);
    assert_eq!(hook_entries(&target), 0);
}

#[test]
fn only_app_origin_from_home() {
    let target = node_with_install("origin", true);
    let (home, worker) = cluster(&target);
    for tier in [OriginTier::Agent, OriginTier::Node] {
        for op in [ops::INSTALL_APPLY, ops::INSTALL_PLAN] {
            let sent = home.node.local(LocalCmd::Forward {
                node: worker.id(),
                op: op.to_string(),
                args: json!({}),
                origin: Origin {
                    node: home.id(),
                    tier,
                },
            });
            assert!(sent.is_err(), "{op} {tier:?}: {sent:?}");
        }
    }
    let agent = Caller {
        tier: Tier::Agent,
        ..caller(ClientKind::Cli)
    };
    let refused = NodeForward::new(home.node.clone())
        .forward(&agent, ops::INSTALL_APPLY, &json!({"node": worker.id()}))
        .expect("routed")
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);

    let from_worker = worker.node.local(LocalCmd::Forward {
        node: home.id(),
        op: ops::INSTALL_APPLY.to_string(),
        args: json!({}),
        origin: Origin {
            node: worker.id(),
            tier: OriginTier::App,
        },
    });
    assert!(from_worker.is_err(), "{from_worker:?}");
    assert!(home.calls.calls.lock().unwrap().is_empty());
    assert_eq!(settings_text(&target.dir), USER_SETTINGS);
}

#[test]
fn plan_shows_before_apply() {
    let target = node_with_install("plan-first", true);
    let (home, worker) = cluster(&target);
    let plan = forward_from(&home, ops::INSTALL_PLAN, &worker).unwrap();
    assert_eq!(plan["applied"], false, "{plan}");
    assert_eq!(plan["plan"]["changed"], true);
    assert!(!plan["plan"]["hooks"]["changes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(settings_text(&target.dir), USER_SETTINGS);
    let applied = forward_from(&home, ops::INSTALL_APPLY, &worker).unwrap();
    assert_eq!(applied["applied"], true);
    assert_eq!(
        applied["plan"]["hooks"]["changes"],
        plan["plan"]["hooks"]["changes"]
    );
}

#[test]
fn an_unreadable_node_is_an_error_and_never_runs_here() {
    let target = node_with_install("bad-node", true);
    let (home, worker) = cluster(&target);
    let forward = NodeForward::new(home.node.clone());
    for node in [json!("m1"), json!(7), json!(""), json!("0000000000000000")] {
        let sent = forward
            .forward(&app(), ops::INSTALL_APPLY, &json!({"node": node}))
            .expect("never falls through to this Mac");
        assert!(sent.is_err(), "{node}: {sent:?}");
    }
    assert!(forward
        .forward(&app(), ops::INSTALL_APPLY, &json!({}))
        .is_none());
    let own = json!({"node": home.id()});
    assert!(forward.forward(&app(), ops::INSTALL_APPLY, &own).is_none());
    assert_eq!(settings_text(&target.dir), USER_SETTINGS);
    drop(worker);
}

#[test]
fn a_call_that_came_from_a_node_never_forwards_again() {
    let target = node_with_install("no-relay", true);
    let (home, worker) = cluster(&target);
    let relayed = Caller {
        client: ClientKind::Node,
        ..app()
    };
    let args = json!({"node": worker.id()});
    assert!(NodeForward::new(home.node.clone())
        .forward(&relayed, ops::INSTALL_APPLY, &args)
        .is_none());
    assert_eq!(settings_text(&target.dir), USER_SETTINGS);
}
