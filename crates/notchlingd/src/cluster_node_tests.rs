use crate::cluster_node::Hooks;
use crate::cluster_testkit::{eventually, member, node, online, pair, Dir, Opts};
use notchling_cluster_net::{Conn, LinkEvents, MemNet, MemTransport, Pins, Transport};
use notchling_engine::cluster::core::{SnapAsk, PEER_STALE_TERM};
use notchling_engine::cluster::snapshot::{Observed, Trigger};
use notchling_engine::cluster::wire::{encode, Msg, Origin, OriginTier, WireError};
use notchling_engine::cluster::{LocalCmd, NodeId, Term};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
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
    assert!(n.audit_text().contains("cluster.step step=save"));
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
            args: json!({"command": "true"}).into(),
            origin,
        })
        .unwrap();
    assert_eq!(out["session"], "s-shell.run");
    let calls = b.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "shell.run");
    assert_eq!(calls[0].2.node, a.id());
    let audit = b.audit_text();
    assert!(audit.contains("cluster.step step=call"), "{audit}");
    assert!(audit.contains(a.id().as_str()), "{audit}");
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
            args: json!({"command": "true"}).into(),
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

#[derive(Default)]
struct Asks(Mutex<Vec<SnapAsk>>);

impl Hooks for Asks {
    fn snapshot(&self, ask: SnapAsk) {
        self.0.lock().unwrap().push(ask);
    }
}

fn can_be_master(a: &crate::cluster_testkit::TestNode, b: &crate::cluster_testkit::TestNode) {
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
}

#[test]
fn a_snapshot_effect_reaches_the_hook_without_blocking_the_driver() {
    let net = MemNet::new();
    let asks = Arc::new(Asks::default());
    let mut home = Opts::named("home");
    home.hooks = Some(asks.clone());
    let (a, b) = pair(&net, home, Opts::named("standby"));
    can_be_master(&a, &b);
    a.node
        .local(LocalCmd::Snapshot {
            trigger: Trigger::Ready(b.id()),
        })
        .unwrap();
    eventually("the hook was asked", || !asks.0.lock().unwrap().is_empty());
    let ask = asks.0.lock().unwrap()[0].clone();
    assert_eq!(ask.trigger, Trigger::Ready(b.id()));
    assert_eq!(ask.targets, vec![b.id()]);
    assert_eq!(ask.home, a.id());
    assert!(
        a.node.status()["me"] == json!(a.id()),
        "the driver still answers"
    );
}

#[test]
fn observed_input_reaches_the_core_and_asks_on_the_timer() {
    let net = MemNet::new();
    let asks = Arc::new(Asks::default());
    let mut home = Opts::named("home");
    home.hooks = Some(asks.clone());
    let (a, b) = pair(&net, home, Opts::named("standby"));
    can_be_master(&a, &b);
    a.node.observe(Observed {
        now_ms: 5_000_000,
        ..Observed::default()
    });
    eventually("a timer set was asked", || {
        !asks.0.lock().unwrap().is_empty()
    });
    assert_eq!(asks.0.lock().unwrap()[0].trigger, Trigger::Timer);
    b.node.observe(Observed {
        now_ms: 9_000_000,
        ..Observed::default()
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(asks.0.lock().unwrap().len(), 1, "a standby never asks");
}

#[test]
fn a_relayed_stale_term_reaches_a_local_caller_as_forbidden() {
    for code in ["stale_term", PEER_STALE_TERM] {
        let err = super::api_error(WireError::new(code, "{}"));
        assert_eq!(
            err.code,
            notchling_local_api::ErrorCode::Forbidden,
            "{code}"
        );
    }
}

struct Silent;

impl LinkEvents for Silent {
    fn opened(&self, _conn: Arc<dyn Conn>) {}

    fn line(&self, _peer_fp: &str, _line: Vec<u8>) {}

    fn closed(&self, _peer_fp: &str, _conn_id: u64) {}
}

fn lookalike(real: &str) -> String {
    format!("{}{}", &real[..16], "e".repeat(48))
}

#[test]
fn a_certificate_that_only_shares_a_members_id_cannot_take_its_place() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home8"), Opts::named("worker8"));
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
    a.node.join_token().unwrap();
    let fake = lookalike(&b.fp);
    assert_ne!(fake, b.fp);
    assert_eq!(NodeId::from_fingerprint(&fake), Some(b.id()));
    let stranger = MemTransport::on(&net, &fake, Pins::default());
    let addrs = a.transport.addrs();
    let mut dialed = None;
    eventually("the join window admits an unpinned certificate", || {
        dialed = stranger.dial(&addrs, 47823, &a.fp, Arc::new(Silent)).ok();
        dialed.is_some()
    });
    let conn = dialed.unwrap();
    let cluster = a.node.status()["cluster"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hello = Msg::Hello {
        node: b.id(),
        cluster: Some(cluster),
        versions: crate::cluster_versions::own(),
        boot_id: "boot".to_string(),
        term: Some(Term { n: 5, node: b.id() }),
    };
    let _ = conn.send(encode(&hello).unwrap());
    eventually("the home closes the look-alike", || !conn.is_open());
    std::thread::sleep(Duration::from_millis(300));
    let status = a.node.status();
    assert_eq!(status["cluster"]["term"]["n"], 1, "{status}");
    assert_eq!(status["cluster"]["home"], json!(a.id()));
    assert!(online(&status, &b.id()), "the real member stays linked");
    assert!(online(&b.node.status(), &a.id()));
}

#[test]
fn a_line_from_a_certificate_that_only_shares_a_members_id_is_ignored() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home9"), Opts::named("worker9"));
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
    let cluster = a.node.status()["cluster"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hello = Msg::Hello {
        node: b.id(),
        cluster: Some(cluster),
        versions: crate::cluster_versions::own(),
        boot_id: "boot".to_string(),
        term: Some(Term { n: 5, node: b.id() }),
    };
    let fake = lookalike(&b.fp);
    let _ = a
        .node
        .tx
        .send(super::In::Line(fake, encode(&hello).unwrap()));
    a.node.join_token().unwrap();
    let status = a.node.status();
    assert_eq!(status["cluster"]["term"]["n"], 1, "{status}");
    assert_eq!(status["cluster"]["home"], json!(a.id()));
    assert!(online(&status, &b.id()));
}
