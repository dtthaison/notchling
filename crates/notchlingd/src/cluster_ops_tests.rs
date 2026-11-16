use super::{follow_switch, ClusterOps, CLUSTER_OPS};
use crate::cluster_testkit::{eventually, member, node, pair, Opts, TestNode};
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, CLUSTER, CLUSTER_OFF};
use notchling_cluster_net::MemNet;
use notchling_local_api::{ops, topics, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: match tier {
            Tier::App => ClientKind::App,
            Tier::Agent => ClientKind::Cli,
        },
        peer: None,
        pid: None,
    }
}

struct Wired {
    ops: ClusterOps,
    flag: Arc<SwitchFlag>,
}

fn wired(n: &TestNode, on: bool) -> Wired {
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    follow_switch(&flag, &n.node);
    if on {
        flag.set(true);
    }
    let ops = ClusterOps::new(
        Ok(n.node.clone()),
        flag.clone(),
        n.room.clone(),
        n.log.clone(),
    );
    Wired { ops, flag }
}

fn token_from(home: &TestNode) -> String {
    home.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn status_is_agent_tier() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("tiers"));
    let w = wired(&n, false);
    let agent = [
        ops::CLUSTER_STATUS,
        ops::CLUSTER_JOIN_TOKEN,
        ops::CLUSTER_JOIN,
    ];
    for op in CLUSTER_OPS {
        let want = if agent.contains(&op) {
            OpSpec::AGENT
        } else {
            OpSpec::APP
        };
        assert_eq!(w.ops.spec(op), Some(want), "{op}");
    }
    assert_eq!(w.ops.spec("node.switch.set"), None);
    assert_eq!(w.ops.topic_tier(topics::CLUSTER), Some(Tier::Agent));
    let status = w
        .ops
        .call(&caller(Tier::Agent), ops::CLUSTER_STATUS, &json!({}))
        .unwrap();
    assert_eq!(status["cluster"], Value::Null);
    assert_eq!(status["me"], json!(n.id()));
}

#[test]
fn a_node_that_could_not_start_says_why() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("broken"));
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    let ops = ClusterOps::new(
        Err("no identity".into()),
        flag,
        n.room.clone(),
        n.log.clone(),
    );
    let status = ops
        .call(&caller(Tier::Agent), ops::CLUSTER_STATUS, &json!({}))
        .unwrap();
    assert_eq!(status["cluster"], Value::Null);
    assert_eq!(status["error"], "no identity");
    assert_eq!(status["joining"], Value::Null);
    let err = ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_CREATE,
            &json!({"name": "c"}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(err.message.contains("no identity"), "{err:?}");
}

#[test]
fn create_makes_this_node_the_home() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("creator"));
    let w = wired(&n, false);
    w.ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_CREATE,
            &json!({"name": " Studio "}),
        )
        .unwrap();
    let status = n.node.status();
    assert_eq!(status["cluster"]["name"], "Studio");
    assert_eq!(status["cluster"]["home"], json!(n.id()));
    let bad = w
        .ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_CREATE,
            &json!({"name": "  "}),
        )
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::BadRequest);
}

#[test]
fn an_agent_join_is_refused_while_the_app_has_the_switch_off() {
    let net = MemNet::new();
    let (home, _worker) = pair(&net, Opts::named("home6"), Opts::named("worker6"));
    let joiner = node(&net, Opts::named("mac6"));
    joiner.room.app.store(true, Ordering::SeqCst);
    let w = wired(&joiner, false);
    let args = json!({"token": token_from(&home)});
    let err = w
        .ops
        .call(&caller(Tier::Agent), ops::CLUSTER_JOIN, &args)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(err.message, CLUSTER_OFF);
    assert!(!w.flag.is_on());
    let joined = w
        .ops
        .call(&caller(Tier::App), ops::CLUSTER_JOIN, &args)
        .unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");
    assert!(w.flag.is_on());
    assert!(joiner
        .audit_text()
        .contains("switch.changed step=- switch=cluster on=true cause=user"));
}

#[test]
fn a_headless_join_turns_the_link_on_and_is_audited() {
    let net = MemNet::new();
    let (home, _worker) = pair(&net, Opts::named("home7"), Opts::named("worker7"));
    let joiner = node(&net, Opts::named("box7"));
    let w = wired(&joiner, false);
    let args = json!({"token": token_from(&home)});
    let joined = w
        .ops
        .call(&caller(Tier::Agent), ops::CLUSTER_JOIN, &args)
        .unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");
    assert!(w.flag.is_on());
    eventually("the link is on", || joiner.node.status()["link"] == true);
    let audit = joiner.audit_text();
    assert!(
        audit.contains("switch.changed step=- switch=cluster on=true cause=cluster_join"),
        "{audit}"
    );
    let switched = joiner.room.named(topics::SWITCH_CHANGED);
    assert_eq!(switched, vec![json!({"switch": "cluster", "on": true})]);
}

#[test]
fn join_refused_when_already_in_a_cluster() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("home8"), Opts::named("worker8"));
    let w = wired(&worker, true);
    let err = w
        .ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_JOIN,
            &json!({"token": token_from(&home)}),
        )
        .unwrap_err();
    assert_eq!(err.message, "this node is already in a cluster");
}

#[test]
fn bad_arguments_are_bad_requests() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("args"));
    let w = wired(&n, false);
    let cases = [
        (ops::CLUSTER_JOIN, json!({})),
        (ops::CLUSTER_REMOVE, json!({"node": "not hex"})),
        (
            ops::CLUSTER_MEMBER_SET,
            json!({"node": "0123456789abcdef", "bogus": 1}),
        ),
        (ops::CLUSTER_MEMBER_SET, json!({})),
    ];
    for (op, args) in cases {
        let err = w.ops.call(&caller(Tier::App), op, &args).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{op} {args}");
    }
}

#[test]
fn member_set_and_remove_reach_the_home() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("home9"), Opts::named("worker9"));
    let w = wired(&home, true);
    w.ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_MEMBER_SET,
            &json!({"node": worker.id(), "acceptsDelegation": true}),
        )
        .unwrap();
    let status = home.node.status();
    let m = member(&status, &worker.id()).unwrap();
    assert_eq!(m["acceptsDelegation"], true, "{m}");
    w.ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_REMOVE,
            &json!({"node": worker.id()}),
        )
        .unwrap();
    eventually("the worker is removed", || {
        member(&home.node.status(), &worker.id()).is_none_or(|m| m["state"] == "removed")
    });
}

#[test]
fn main_set_takes_a_session_and_a_flag() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainargs"));
    let w = wired(&n, false);
    let cases = [
        json!({}),
        json!({"id": "job-1"}),
        json!({"main": false}),
        json!({"id": "job-1", "main": "no"}),
        json!({"id": "a".repeat(257), "main": false}),
        json!({"id": "a\nb", "main": false}),
    ];
    for args in cases {
        let err = w
            .ops
            .call(&caller(Tier::App), ops::CLUSTER_MAIN_SET, &args)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{args}");
    }
}

#[test]
fn main_set_on_the_home_keeps_a_session_here() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainhome9"));
    n.node.create("home").unwrap();
    let w = wired(&n, true);
    w.ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_MAIN_SET,
            &json!({"id": "job-1", "main": false}),
        )
        .unwrap();
    assert_eq!(n.node.status()["kept"], json!(["job-1"]));
    w.ops
        .call(
            &caller(Tier::App),
            ops::CLUSTER_MAIN_SET,
            &json!({"id": "job-1", "main": true}),
        )
        .unwrap();
    assert_eq!(n.node.status()["kept"], json!([]));
}

#[test]
fn main_set_from_a_client_reaches_the_home() {
    let net = MemNet::new();
    let (home, client) = pair(&net, Opts::named("home10"), Opts::named("client10"));
    home.node
        .local(notchling_engine::cluster::LocalCmd::SetMember {
            node: client.id(),
            can_be_master: None,
            client: Some(true),
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
    let w = wired(&client, false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let sent = w.ops.call(
            &caller(Tier::App),
            ops::CLUSTER_MAIN_SET,
            &json!({"id": "job-1", "main": false}),
        );
        if sent.is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the client's change never reached the home: {sent:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(home.node.status()["kept"], json!(["job-1"]));
}

#[test]
fn status_carries_what_replication_reports() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("replication"));
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    follow_switch(&flag, &n.node);
    let ops = ClusterOps::new(Ok(n.node.clone()), flag, n.room.clone(), n.log.clone())
        .with_replication(Arc::new(|_| json!([{"node": "x", "setAgeS": 7}])));
    let status = ops
        .call(&caller(Tier::Agent), ops::CLUSTER_STATUS, &json!({}))
        .unwrap();
    assert_eq!(status["replication"], json!([{"node": "x", "setAgeS": 7}]));
}

fn node_caller() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    }
}

#[test]
fn a_token_asked_by_another_node_must_name_its_maker() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("tokenhome"), Opts::named("tokenbox"));
    let w = wired(&home, true);
    for args in [
        json!({}),
        json!({"minter": ""}),
        json!({"minter": "not-a-node"}),
        json!({"minter": 7}),
    ] {
        let err = w
            .ops
            .call(&node_caller(), ops::CLUSTER_JOIN_TOKEN, &args)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{args}");
    }
    let made = w
        .ops
        .call(
            &node_caller(),
            ops::CLUSTER_JOIN_TOKEN,
            &json!({"minter": worker.id()}),
        )
        .unwrap();
    assert!(made["token"]
        .as_str()
        .is_some_and(|t| t.starts_with("nlj1.")));
}

#[test]
fn a_local_callers_args_never_name_the_maker() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("tokenhome2"), Opts::named("tokenbox2"));
    let w = wired(&home, true);
    let made = w
        .ops
        .call(
            &caller(Tier::Agent),
            ops::CLUSTER_JOIN_TOKEN,
            &json!({"minter": worker.id()}),
        )
        .unwrap();
    assert!(made["token"]
        .as_str()
        .is_some_and(|t| t.starts_with("nlj1.")));
}

#[test]
fn allow_set_replaces_the_names_and_keeps_what_it_is_not_told() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("home10"), Opts::named("worker10"));
    let w = wired(&home, true);
    let set = |args: Value| {
        w.ops
            .call(&caller(Tier::App), ops::CLUSTER_ALLOW_SET, &args)
            .unwrap()
    };
    set(json!({"node": worker.id(), "names": ["GH", "db"]}));
    let status = home.node.status();
    let m = member(&status, &worker.id()).unwrap();
    assert_eq!(m["secrets"]["names"], json!(["db", "gh"]), "{m}");
    assert_eq!(m["secrets"]["any"], false);
    set(json!({"node": worker.id(), "any": true}));
    let status = home.node.status();
    let m = member(&status, &worker.id()).unwrap();
    assert_eq!(m["secrets"]["names"], json!(["db", "gh"]), "{m}");
    assert_eq!(m["secrets"]["any"], true);
    set(json!({"node": worker.id(), "names": [], "any": false}));
    let status = home.node.status();
    let m = member(&status, &worker.id()).unwrap();
    assert_eq!(m["secrets"], json!({"names": [], "any": false}), "{m}");
}

#[test]
fn allow_set_is_app_tier_and_checks_its_arguments() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("home11"), Opts::named("worker11"));
    let w = wired(&home, true);
    assert_eq!(w.ops.spec(ops::CLUSTER_ALLOW_SET), Some(OpSpec::APP));
    let cases = [
        json!({}),
        json!({"node": worker.id(), "names": "gh"}),
        json!({"node": worker.id(), "names": [1]}),
        json!({"node": worker.id(), "any": "yes"}),
        json!({"node": worker.id(), "bogus": true}),
    ];
    for args in cases {
        let e = w
            .ops
            .call(&caller(Tier::App), ops::CLUSTER_ALLOW_SET, &args)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
    }
    let spaced = json!({"node": worker.id(), "names": ["has space"]});
    let e = w
        .ops
        .call(&caller(Tier::App), ops::CLUSTER_ALLOW_SET, &spaced)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    let status = home.node.status();
    let m = member(&status, &worker.id()).unwrap();
    assert_eq!(m["secrets"], json!({"names": [], "any": false}), "{m}");
}
