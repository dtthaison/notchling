use crate::cluster_testkit::{eventually, member, node, online, pair, Dir, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::LocalCmd;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn two_nodes_link_and_see_each_other() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("worker"));
    let sa = a.node.status();
    let sb = b.node.status();
    assert_eq!(sa["cluster"]["id"], sb["cluster"]["id"]);
    assert_eq!(sa["cluster"]["home"], json!(a.id()));
    assert_eq!(member(&sa, &a.id()).unwrap()["role"], "home");
    assert_eq!(member(&sa, &b.id()).unwrap()["role"], "worker");
    assert!(online(&sb, &b.id()), "a node is online to itself");
    assert_eq!(sa["link"], true);
    assert!(a.cluster_dir().join("cluster.db").exists());
    assert!(b.cluster_dir().join("cluster.db").exists());
    assert!(
        !a.room.named("cluster.changed").is_empty(),
        "changes are published"
    );
}

#[test]
fn status_without_cluster_is_null() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("alone"));
    let status = n.node.status();
    assert_eq!(status["cluster"], Value::Null);
    assert_eq!(status["me"], json!(n.id()));
    assert_eq!(status["members"], json!([]));
}

#[test]
fn the_link_starts_off_and_binds_nothing() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("off"));
    n.node.create("c").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(n.transport.listens.load(Ordering::SeqCst), 0);
    assert_eq!(n.node.status()["link"], false);
    n.node.set_link(true);
    eventually("listening", || {
        n.transport.listens.load(Ordering::SeqCst) == 1
    });
    n.node.set_link(false);
    eventually("off", || n.node.status()["link"] == false);
}

#[test]
fn fixture_root_binds_nothing() {
    let net = MemNet::new();
    let mut opts = Opts::named("fixture");
    opts.fixture = true;
    let n = node(&net, opts);
    n.node.set_link(true);
    n.node.create("c").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(n.transport.listens.load(Ordering::SeqCst), 0);
    assert_eq!(n.transport.dials.load(Ordering::SeqCst), 0);
    assert_eq!(n.node.status()["listening"], Value::Null);
}

#[test]
fn failed_save_fails_the_op_and_keeps_state() {
    let net = MemNet::new();
    let blocker = Dir::new("blocker");
    let file = blocker.path().join("not-a-folder");
    std::fs::write(&file, b"x").unwrap();
    let mut opts = Opts::named("nosave");
    opts.cluster_dir = Some(file.clone());
    let n = node(&net, opts);
    let err = n.node.create("c").unwrap_err();
    assert!(err.message.contains("saving the cluster failed"), "{err:?}");
    let status = n.node.status();
    assert_eq!(status["cluster"], Value::Null, "{status}");
    assert!(status["error"].is_string(), "{status}");
    assert!(status["error"]
        .as_str()
        .unwrap()
        .contains("saving the cluster failed"));
    assert_eq!(std::fs::read(&file).unwrap(), b"x");
    assert!(n.audit_text().contains("cluster:save"));
}

#[test]
fn a_restart_keeps_the_cluster() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home2"), Opts::named("worker2"));
    let id = a.node.status()["cluster"]["id"].clone();
    a.node.close();
    let state = notchling_engine::cluster::persist::load(&a.cluster_dir())
        .unwrap()
        .unwrap();
    assert_eq!(json!(state.id), id);
    assert!(state.members.contains_key(&b.id()));
}

#[test]
fn a_join_with_a_bad_token_is_a_bad_request() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("badtoken"));
    n.node.set_link(true);
    let err = n.node.join("nlj1.garbage", false).unwrap_err();
    assert_eq!(err.code, notchling_local_api::ErrorCode::BadRequest);
}

#[test]
fn a_join_with_the_link_off_is_refused() {
    let net = MemNet::new();
    let (a, _b) = pair(&net, Opts::named("home3"), Opts::named("worker3"));
    let token = a.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let c = node(&net, Opts::named("late"));
    let err = c.node.join(&token, false).unwrap_err();
    assert!(err.message.contains("links are off"), "{err:?}");
}

#[test]
fn the_home_runs_an_allowed_op_on_a_worker() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home4"), Opts::named("worker4"));
    let origin = Origin {
        node: a.id(),
        tier: OriginTier::Node,
    };
    let out = a
        .node
        .local(LocalCmd::Forward {
            node: b.id(),
            op: "shell.run".to_string(),
            args: json!({"command": "true"}),
            origin,
        })
        .unwrap();
    assert_eq!(out["session"], "s-shell.run");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "shell.run");
    assert_eq!(calls[0].2.node, a.id());
    assert!(b.audit_text().contains("cluster:shell.run"));
}

#[test]
fn a_worker_cannot_run_ops_on_the_home() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home5"), Opts::named("worker5"));
    let origin = Origin {
        node: b.id(),
        tier: OriginTier::Node,
    };
    let err = b
        .node
        .local(LocalCmd::Forward {
            node: a.id(),
            op: "shell.run".to_string(),
            args: json!({"command": "true"}),
            origin,
        })
        .unwrap_err();
    assert_eq!(
        err.code,
        notchling_local_api::ErrorCode::Forbidden,
        "{err:?}"
    );
    assert!(a.calls.calls.lock().unwrap().is_empty());
}
