use crate::acting_ops_tests::FakeRoom;
use crate::cluster_ops::ClusterOps;
use crate::cluster_promote_kit::{rig, Rig};
use crate::cluster_testkit::{node, pair, Opts, TestNode};
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

fn code(result: Result<Value, ApiError>) -> ErrorCode {
    result.unwrap_err().code
}

#[test]
fn agent_cannot_promote() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("solo"));
    let handler = Plain(Arc::new(ops_on(&n)));
    assert_eq!(handler.spec(ops::CLUSTER_PROMOTE), Some(OpSpec::APP));
    let room = FakeRoom::default();
    let args = json!({"node": n.id()});
    assert_eq!(
        code(checked(
            &handler,
            &room,
            &agent(),
            ops::CLUSTER_PROMOTE,
            &args
        )),
        ErrorCode::Forbidden
    );
    let from_agent = Origin {
        node: n.id(),
        tier: OriginTier::Agent,
    };
    assert_eq!(
        code(checked(
            &handler,
            &room,
            &node_caller(&from_agent),
            ops::CLUSTER_PROMOTE,
            &args
        )),
        ErrorCode::Forbidden
    );
}

#[test]
fn promote_takes_a_node_and_nothing_else() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("solo"));
    let o = ops_on(&n);
    for bad in [
        json!({}),
        json!({"node": "nope"}),
        json!({"node": n.id(), "extra": 1}),
        json!({"node": n.id(), "confirmed": "yes"}),
        json!("text"),
    ] {
        let result = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_PROMOTE, &bad);
        assert_eq!(code(result), ErrorCode::BadRequest, "{bad}");
    }
}

#[test]
fn a_machine_without_a_promoter_says_so() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("solo"));
    let o = ops_on(&n);
    let args = json!({"node": n.id()});
    let result = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_PROMOTE, &args);
    assert!(result.unwrap_err().message.contains("can't be promoted"));
}

#[test]
fn forwarded_promote_runs_on_the_named_node() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("worker"));
    let o = ops_on(&a);
    let args = json!({"node": b.id(), "confirmed": true});
    let answer = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_PROMOTE, &args).unwrap();
    assert_eq!(answer, json!({"session": "s-cluster.promote"}));
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "cluster.promote");
    assert_eq!(calls[0].1, args);
    assert_eq!(calls[0].2.node, a.id());
    assert_eq!(calls[0].2.tier, OriginTier::App);
}

#[test]
fn a_promote_for_this_node_runs_here() {
    let r = rig();
    let b = r.b();
    let net = MemNet::new();
    let mut opts = Opts::named("standby");
    opts.fingerprint = Some(format!("{:02x}", 2u8).repeat(32));
    let n = node(&net, opts);
    assert_eq!(n.id(), b);
    let o = ops_on(&n).with_promoter(r.promoter.clone());
    r.answer(&r.a(), "node.load", Ok(json!({})));
    let args = json!({"node": b});
    let answer = crate::op_group::OpGroup::call(&o, &app(), ops::CLUSTER_PROMOTE, &args).unwrap();
    assert_eq!(answer["relayed"], true, "{answer}");
    r.finish();
}

fn status_of(r: &Rig, o: &ClusterOps) -> Value {
    r.finish();
    crate::op_group::OpGroup::call(o, &app(), ops::CLUSTER_STATUS, &json!({})).unwrap()
}

#[test]
fn status_carries_the_promote_and_no_takeover_time() {
    let r = rig();
    let net = MemNet::new();
    let n = node(&net, Opts::named("solo"));
    n.node.create("home").unwrap();
    let o = ops_on(&n).with_promoter(r.promoter.clone());
    let before = status_of(&r, &o);
    assert_eq!(before["promote"], Value::Null);
    assert_eq!(before["failover"]["takesOverAt"], Value::Null);
    r.answer(&r.a(), "node.load", Ok(json!({})));
    r.promoter.start(false).unwrap();
    let shown = status_of(&r, &o);
    assert_eq!(shown["promote"]["step"], "relayed");
    assert_eq!(shown["promote"]["node"], json!(r.b()));
}
