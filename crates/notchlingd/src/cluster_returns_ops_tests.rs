use crate::acting_ops_tests::FakeRoom;
use crate::cluster_fence_keeper_kit::{node, S1};
use crate::cluster_ops::ClusterOps;
use crate::cluster_returns_kit::{kit, seed_after_promote};
use crate::cluster_testkit::{node as test_node, Opts, TestNode};
use crate::cluster_wiring::{checked, node_caller};
use crate::switches::{SwitchFlag, CLUSTER, CLUSTER_OFF};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Handler, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

struct Plain(Arc<ClusterOps>);

impl Handler for Plain {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        crate::op_group::OpGroup::spec(&*self.0, op)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        crate::op_group::OpGroup::call(&*self.0, caller, op, args)
    }
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

fn ops_on(n: &TestNode) -> ClusterOps {
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    ClusterOps::new(Ok(n.node.clone()), flag, n.room.clone(), n.log.clone())
}

fn forbidden(result: Result<Value, ApiError>) -> bool {
    result.is_err_and(|e| e.code == ErrorCode::Forbidden)
}

#[test]
fn agent_cannot_take_back_or_continue() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    let handler = Plain(Arc::new(ops_on(&n)));
    let room = FakeRoom::default();
    let from_agent = Origin {
        node: n.id(),
        tier: OriginTier::Agent,
    };
    let calls = [
        (ops::CLUSTER_TAKE_BACK, json!({})),
        (ops::CLUSTER_CONTINUE_HERE, json!({"id": S1})),
        (
            ops::CLUSTER_SIDE_COPY_HIDE,
            json!({"session": S1, "node": n.id()}),
        ),
    ];
    for (op, args) in calls {
        assert_eq!(handler.spec(op), Some(OpSpec::APP), "{op}");
        assert!(
            forbidden(checked(&handler, &room, &agent(), op, &args)),
            "{op}"
        );
        assert!(
            forbidden(checked(
                &handler,
                &room,
                &node_caller(&from_agent),
                op,
                &args
            )),
            "{op} from a node's agent"
        );
    }
}

#[test]
fn the_ops_take_exactly_their_arguments() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    let k = kit("ops-args", seed_after_promote(&[]));
    let o = ops_on(&n).with_returns(k.returns.clone());
    let me = n.id();
    let bad = [
        (ops::CLUSTER_TAKE_BACK, json!({"extra": 1})),
        (ops::CLUSTER_TAKE_BACK, json!("text")),
        (ops::CLUSTER_CONTINUE_HERE, json!({})),
        (ops::CLUSTER_CONTINUE_HERE, json!({"id": ""})),
        (ops::CLUSTER_CONTINUE_HERE, json!({"id": S1, "x": 1})),
        (ops::CLUSTER_SIDE_COPY_HIDE, json!({"session": S1})),
        (
            ops::CLUSTER_SIDE_COPY_HIDE,
            json!({"session": S1, "node": "nope"}),
        ),
        (
            ops::CLUSTER_SIDE_COPY_HIDE,
            json!({"session": S1, "node": me, "x": 1}),
        ),
    ];
    for (op, args) in bad {
        let result = crate::op_group::OpGroup::call(&o, &app(), op, &args);
        assert_eq!(
            result.unwrap_err().code,
            ErrorCode::BadRequest,
            "{op} {args}"
        );
    }
}

#[test]
fn a_machine_without_returns_says_so() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    let o = ops_on(&n);
    let result = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_TAKE_BACK, &json!({}));
    assert!(result.unwrap_err().message.contains("can't take back"));
}

#[test]
fn take_back_op_runs_the_handover() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    let k = kit("ops-take", seed_after_promote(&[(S1, 1)]));
    let o = ops_on(&n).with_returns(k.returns.clone());
    let answer = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_TAKE_BACK, &json!({}))
        .expect("taken back");
    assert_eq!(answer, json!({"started": true}));
    assert_eq!(k.link.app_calls.lock().unwrap()[0].0, node("m1"));
}

#[test]
fn status_lists_waiting() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    n.node.create("home").unwrap();
    let k = kit("ops-status", seed_after_promote(&[(S1, 1)]));
    k.rows
        .0
        .lock()
        .unwrap()
        .push(crate::cluster_returns_kit::agent_row(
            S1,
            "Fix the build",
            notchling_engine::sessions::AgentGroup::Completed,
        ));
    let o = ops_on(&n).with_returns(k.returns.clone());
    let none = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_STATUS, &json!({})).unwrap();
    assert_eq!(none["waiting"], json!([]));
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    let shown =
        crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_STATUS, &json!({})).unwrap();
    assert_eq!(
        shown["waiting"],
        json!([{"session": S1, "node": node("m1"), "name": "m1", "title": "Fix the build"}])
    );
    let bare = ops_on(&n);
    let plain =
        crate::op_group::OpGroup::call(&bare, &app(), ops::CLUSTER_STATUS, &json!({})).unwrap();
    assert_eq!(plain["waiting"], json!([]));
}

#[test]
fn hide_and_continue_ops_reach_the_returns() {
    let net = MemNet::new();
    let n = test_node(&net, Opts::named("solo"));
    let k = kit("ops-hide", seed_after_promote(&[(S1, 1)]));
    let path = k.write_transcript(S1, 20);
    k.side_copy(S1, &path);
    let o = ops_on(&n).with_returns(k.returns.clone());
    let args = json!({"session": S1, "node": node("m5")});
    crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_SIDE_COPY_HIDE, &args).unwrap();
    assert!(k.state().side_copies[0].hidden);
    k.returns.add_waiting(S1, &node("m1")).unwrap();
    let args = json!({"id": S1});
    crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_CONTINUE_HERE, &args).unwrap();
    assert_eq!(k.state().claims[S1].node, node("m5"));
}
