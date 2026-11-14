use super::{join_item, JoinAsker, JOIN_WAIT};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{Hooks, JoinAsk, Submit};
use crate::cluster_testkit::{eventually, member, node, online, Dir, Opts, TestNode};
use crate::inbox_ops::InboxOps;
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::join::decode_token;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct Asks(JoinAsker);

impl Hooks for Asks {
    fn join_request(&self, ask: JoinAsk, node: Submit) {
        self.0.ask(ask, node);
    }
}

struct Home {
    node: TestNode,
    inbox: Arc<InboxOps>,
    _dir: Dir,
}

fn home(net: &Arc<MemNet>, name: &'static str, wait: Duration) -> Home {
    let dir = Dir::new(&format!("{name}-inbox"));
    let inbox = InboxOps::new(dir.path().to_path_buf(), Arc::new(FakeRoom::default()));
    let mut opts = Opts::named(name);
    opts.hooks = Some(Arc::new(Asks(JoinAsker::new(inbox.clone(), wait))));
    let node = node(net, opts);
    node.node.set_link(true);
    node.node.create("home").unwrap();
    Home {
        node,
        inbox,
        _dir: dir,
    }
}

fn joiner(net: &Arc<MemNet>, name: &'static str, os: &'static str) -> TestNode {
    let mut opts = Opts::named(name);
    opts.os = os;
    let n = node(net, opts);
    n.node.set_link(true);
    n
}

fn token(home: &TestNode) -> String {
    home.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn joins(home: &Home) -> Vec<Item> {
    home.inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::NodeJoin)
        .collect()
}

fn asked(home: &Home) -> Item {
    eventually("a join item", || !joins(home).is_empty());
    joins(home).remove(0)
}

#[test]
fn join_with_token_raises_inbox_item_and_allow_adds_worker() {
    let net = MemNet::new();
    let h = home(&net, "jhome1", JOIN_WAIT);
    let w = joiner(&net, "jbox1", "linux");
    let joined = w.node.join(&token(&h.node), false).unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");
    let item = asked(&h);
    assert_eq!(item.title, "Add jbox1 to the cluster");
    assert!(item.detail.starts_with("linux "), "{}", item.detail);
    assert!(item.detail.contains(w.id().as_str()), "{}", item.detail);
    assert!(!item.detail.contains("Can Be Master"));
    assert!(item.id.starts_with(&format!("node-join:{}:", w.id())));
    eventually("the joiner is pending at the home", || {
        member(&h.node.node.status(), &w.id()).is_some_and(|m| m["state"] == "pending")
    });
    let outcome = h.inbox.decide(&item.id, ActionId::Allow, "mac").unwrap();
    assert!(outcome.answered);
    eventually("the worker is active", || {
        online(&h.node.node.status(), &w.id()) && online(&w.node.status(), &h.node.id())
    });
    let status = h.node.node.status();
    let m = member(&status, &w.id()).unwrap();
    assert_eq!(m["role"], "worker");
    assert_eq!(m["canBeMaster"], false);
    assert_eq!(m["client"], false);
    assert_eq!(m["headless"], false);
    assert_eq!(w.node.status()["cluster"]["id"], status["cluster"]["id"]);
}

#[test]
fn a_headless_join_says_so_and_the_home_records_it() {
    let net = MemNet::new();
    let h = home(&net, "jhome9", JOIN_WAIT);
    let w = joiner(&net, "jbox9", "linux");
    w.node.join(&token(&h.node), true).unwrap();
    let item = asked(&h);
    assert!(item.detail.contains("Headless"), "{}", item.detail);
    h.inbox.decide(&item.id, ActionId::Allow, "mac").unwrap();
    eventually("the worker is active", || {
        online(&h.node.node.status(), &w.id()) && online(&w.node.status(), &h.node.id())
    });
    let status = h.node.node.status();
    let m = member(&status, &w.id()).unwrap();
    assert_eq!(m["headless"], true);
    assert_eq!(m["client"], false);
    let mine = w.node.status();
    assert_eq!(member(&mine, &w.id()).unwrap()["headless"], true);
}

#[test]
fn allowed_mac_joins_with_can_be_master_on() {
    let net = MemNet::new();
    let h = home(&net, "jhome2", JOIN_WAIT);
    let w = joiner(&net, "jmac2", "macos");
    w.node.join(&token(&h.node), false).unwrap();
    let item = asked(&h);
    assert!(item.detail.ends_with(" · Can Be Master"), "{}", item.detail);
    h.inbox.decide(&item.id, ActionId::Allow, "mac").unwrap();
    eventually("the Mac is active", || {
        online(&h.node.node.status(), &w.id())
    });
    let status = h.node.node.status();
    assert_eq!(member(&status, &w.id()).unwrap()["canBeMaster"], true);
    assert_eq!(member(&status, &w.id()).unwrap()["role"], "standby");
}

#[test]
fn deny_refuses_the_joiner() {
    let net = MemNet::new();
    let h = home(&net, "jhome3", JOIN_WAIT);
    let w = joiner(&net, "jbox3", "linux");
    w.node.join(&token(&h.node), false).unwrap();
    let item = asked(&h);
    h.inbox.decide(&item.id, ActionId::Deny, "mac").unwrap();
    eventually("the joiner has no cluster", || {
        w.node.status()["cluster"] == Value::Null
    });
    let status = h.node.node.status();
    assert!(member(&status, &w.id()).is_none_or(|m| m["state"] != "active"));
}

#[test]
fn only_the_mac_or_a_phone_may_allow_a_join() {
    let net = MemNet::new();
    let h = home(&net, "jhome4", JOIN_WAIT);
    let w = joiner(&net, "jbox4", "linux");
    w.node.join(&token(&h.node), false).unwrap();
    let item = asked(&h);
    eventually("the joiner is pending at the home", || {
        member(&h.node.node.status(), &w.id()).is_some_and(|m| m["state"] == "pending")
    });
    let err = h
        .inbox
        .decide(&item.id, ActionId::Allow, "agent")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(err.message.contains("into the cluster"), "{err:?}");
    let status = h.node.node.status();
    assert_eq!(member(&status, &w.id()).unwrap()["state"], "pending");
}

#[test]
fn token_is_one_time() {
    let net = MemNet::new();
    let h = home(&net, "jhome5", JOIN_WAIT);
    let once = token(&h.node);
    let a = joiner(&net, "jbox5a", "linux");
    a.node.join(&once, false).unwrap();
    asked(&h);
    let _open = token(&h.node);
    let b = joiner(&net, "jbox5b", "linux");
    let started = std::time::Instant::now();
    let second = b.node.join(&once, false).unwrap_err();
    assert!(second.message.contains("bad token"), "{second:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the refusal comes at once"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(joins(&h).len(), 1);
    assert!(member(&h.node.node.status(), &b.id()).is_none());
}

#[test]
fn join_token_from_client_node_is_minted_by_home() {
    let net = MemNet::new();
    let h = home(&net, "jhome6", JOIN_WAIT);
    let w = joiner(&net, "jbox6", "linux");
    w.node.join(&token(&h.node), false).unwrap();
    let item = asked(&h);
    h.inbox.decide(&item.id, ActionId::Allow, "mac").unwrap();
    eventually("active", || online(&w.node.status(), &h.node.id()));
    let answer = w.node.join_token().unwrap();
    assert_eq!(answer, json!({"session": "s-cluster.join.token"}));
    let calls = h.node.calls.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "cluster.join.token");
    assert_eq!(calls[0].2.node, w.id());
    assert_eq!(calls[0].2.tier, OriginTier::Agent);
    let minted = token(&h.node);
    assert_eq!(decode_token(&minted).unwrap().home, h.node.id());
    let late = joiner(&net, "jbox6b", "linux");
    late.node.join(&minted, false).unwrap();
    eventually("a second item", || joins(&h).len() == 2);
}

#[test]
fn an_unanswered_join_expires_and_is_denied() {
    let net = MemNet::new();
    let h = home(&net, "jhome7", Duration::from_millis(300));
    let w = joiner(&net, "jbox7", "linux");
    w.node.join(&token(&h.node), false).unwrap();
    let item = asked(&h);
    eventually("expired", || {
        joins(&h)
            .iter()
            .any(|i| i.id == item.id && i.state == State::Expired)
    });
    eventually("the joiner has no cluster", || {
        w.node.status()["cluster"] == Value::Null
    });
    let late = h.inbox.decide(&item.id, ActionId::Allow, "mac").unwrap();
    assert!(!late.answered);
}

#[test]
fn the_item_names_the_node_plainly() {
    let ask = JoinAsk {
        item: "node-join:0123456789abcdef".to_string(),
        node: NodeId::parse("0123456789abcdef").unwrap(),
        name: "evil\u{1b}[31m\nname".to_string(),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        addr: Some("100.64.0.9".to_string()),
        can_be_master: false,
        headless: false,
    };
    let item = join_item("x", &ask, 1_000, Duration::from_secs(600));
    assert_eq!(item.title, "Add evil[31mname to the cluster");
    assert_eq!(item.detail, "linux x86_64 · 100.64.0.9 · 0123456789abcdef");
    assert_eq!(item.expires_ms, Some(601_000));
    assert_eq!(
        serde_json::to_value(&item.actions).unwrap(),
        json!([
            {"id": "allow", "label": "Add", "primary": true},
            {"id": "deny", "label": "Deny", "primary": false},
        ])
    );
    assert_eq!(serde_json::to_value(item.kind).unwrap(), "node_join");
}
