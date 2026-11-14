use super::rejoin_on_start;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::Hooks;
use crate::cluster_ops::follow_switch;
use crate::cluster_switches::{HeadlessSwitches, SwitchPanel};
use crate::cluster_testkit::{eventually, node, online, pair, Opts, TestNode};
use crate::switches::SwitchFlag;
use notchling_cluster_net::{MemNet, Transport};
use notchling_engine::cluster::persist::CLUSTER_DB;
use notchling_engine::cluster::{Desired, LocalCmd, NodeId};
use notchling_local_api::{topics, ApiError};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Panel {
    on: Mutex<BTreeMap<String, bool>>,
}

impl SwitchPanel for Panel {
    fn get(&self, switch: &str) -> Option<bool> {
        Some(*self.on.lock().unwrap().get(switch).unwrap_or(&false))
    }

    fn set(&self, switch: &str, on: bool) -> Result<(), ApiError> {
        self.on.lock().unwrap().insert(switch.to_string(), on);
        Ok(())
    }
}

struct Follows(Arc<HeadlessSwitches>);

impl Hooks for Follows {
    fn desired(&self, desired: Desired) {
        self.0.follow(desired);
    }
}

struct Restarted {
    node: TestNode,
    link: Arc<SwitchFlag>,
    panel: Arc<Panel>,
    on: bool,
}

fn cluster_link() -> Arc<SwitchFlag> {
    SwitchFlag::off("cluster", "cluster links are off")
}

fn restart(net: &Arc<MemNet>, old: &TestNode, name: &'static str, app: bool) -> Restarted {
    old.node.close();
    let room = Arc::new(FakeRoom::default());
    room.app.store(app, Ordering::SeqCst);
    let panel = Arc::new(Panel::default());
    let (_, switch_log) = crate::cluster_testkit::test_log();
    let switches = Arc::new(HeadlessSwitches::new(
        panel.clone(),
        room.clone(),
        switch_log,
    ));
    let mut opts = Opts::named(name);
    opts.fingerprint = Some(old.fp.clone());
    opts.cluster_dir = Some(old.cluster_dir());
    opts.hooks = Some(Arc::new(Follows(switches)));
    opts.room = Some(room);
    let node = node(net, opts);
    let link = cluster_link();
    follow_switch(&link, &node.node);
    let on = rejoin_on_start(&link, &node.node, node.room.as_ref(), &node.log);
    Restarted {
        node,
        link,
        panel,
        on,
    }
}

fn want_acting(home: &TestNode, worker: &NodeId) {
    home.node
        .local(LocalCmd::SetMember {
            node: worker.clone(),
            can_be_master: None,
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: Some(Desired {
                acting: true,
                bridge: false,
                install: false,
            }),
        })
        .unwrap();
}

fn members(
    net: &Arc<MemNet>,
    home: &'static str,
    worker: &'static str,
    headless: bool,
) -> (TestNode, TestNode) {
    let mut h = Opts::named(home);
    h.fingerprint = Some("e".repeat(64));
    let mut w = Opts::named(worker);
    w.fingerprint = Some("1".repeat(64));
    w.headless = headless;
    let (h, w) = pair(net, h, w);
    want_acting(&h, &w.id());
    (h, w)
}

fn headless_pair(
    net: &Arc<MemNet>,
    home: &'static str,
    worker: &'static str,
) -> (TestNode, TestNode) {
    members(net, home, worker, true)
}

fn tamper_home(worker: &TestNode, home: &NodeId, path: &str, value: serde_json::Value) {
    let db = rusqlite::Connection::open(worker.cluster_dir().join(CLUSTER_DB)).unwrap();
    let changed = db
        .execute(
            "UPDATE members SET value = json_set(value, ?1, json(?2)) WHERE id = ?3",
            rusqlite::params![path, value.to_string(), home.as_str()],
        )
        .unwrap();
    assert_eq!(changed, 1);
}

fn stays_apart(r: &Restarted, home: &NodeId) {
    eventually("the restarted node tries its home", || {
        r.node.transport.dials.load(Ordering::SeqCst) >= 1
    });
    std::thread::sleep(Duration::from_millis(400));
    assert!(!online(&r.node.node.status(), home));
    assert_eq!(r.panel.get("acting"), Some(false));
}

#[test]
fn a_headless_member_dials_home_on_restart_and_follows_its_switches() {
    let net = MemNet::new();
    let (home, worker) = headless_pair(&net, "rjhome1", "rjw1");
    let r = restart(&net, &worker, "rjw1b", false);
    assert!(r.on);
    assert!(r.link.is_on());
    eventually("the restarted node is back with its home", || {
        online(&r.node.node.status(), &home.id()) && online(&home.node.status(), &r.node.id())
    });
    eventually("acting follows the home", || {
        r.panel.get("acting") == Some(true)
    });
    assert_eq!(r.panel.get("vault"), Some(false));
    assert_eq!(r.panel.get("accounts"), Some(false));
    let audit = r.node.audit_text();
    assert!(
        audit.contains("switch.changed step=- switch=cluster on=true cause=cluster_rejoin"),
        "{audit}"
    );
    assert!(audit.contains("cluster.step step=rejoined"), "{audit}");
    assert!(audit.contains(home.id().as_str()), "{audit}");
    assert_eq!(
        r.node.room.named(topics::SWITCH_CHANGED),
        vec![json!({"switch": "cluster", "on": true})]
    );
}

#[test]
fn a_node_in_no_cluster_stays_off() {
    let net = MemNet::new();
    let lone = node(&net, Opts::named("rjlone"));
    let r = restart(&net, &lone, "rjlone2", false);
    assert!(!r.on);
    assert!(!r.link.is_on());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.node.transport.dials.load(Ordering::SeqCst), 0);
    assert_eq!(r.node.transport.listens.load(Ordering::SeqCst), 0);
    assert!(r.node.audit_text().is_empty());
}

#[test]
fn a_member_joined_from_the_app_stays_off() {
    let net = MemNet::new();
    let (home, worker) = members(&net, "rjhome3", "rjw3", false);
    let r = restart(&net, &worker, "rjw3b", false);
    assert!(!r.on);
    assert!(!r.link.is_on());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.node.transport.dials.load(Ordering::SeqCst), 0);
    assert!(!online(&r.node.node.status(), &home.id()));
    assert_eq!(r.panel.get("acting"), Some(false));
}

#[test]
fn a_headless_member_with_an_app_attached_stays_off() {
    let net = MemNet::new();
    let (home, worker) = headless_pair(&net, "rjhome4", "rjw4");
    let r = restart(&net, &worker, "rjw4b", true);
    assert!(!r.on);
    assert!(!r.link.is_on());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.node.transport.dials.load(Ordering::SeqCst), 0);
    assert!(!online(&r.node.node.status(), &home.id()));
    assert!(r.node.audit_text().is_empty());
}

#[test]
fn a_tampered_home_certificate_fails_closed() {
    let net = MemNet::new();
    let (home, worker) = headless_pair(&net, "rjhome5", "rjw5");
    let impostor = node(&net, Opts::named("rjfake5"));
    impostor.node.set_link(true);
    worker.node.close();
    tamper_home(&worker, &home.id(), "$.fingerprint", json!(impostor.fp));
    let r = restart(&net, &worker, "rjw5b", false);
    assert!(r.on);
    stays_apart(&r, &home.id());
    assert!(!online(&home.node.status(), &r.node.id()));
}

#[test]
fn a_tampered_home_address_fails_closed() {
    let net = MemNet::new();
    let (home, worker) = headless_pair(&net, "rjhome6", "rjw6");
    let impostor = node(&net, Opts::named("rjfake6"));
    impostor.node.set_link(true);
    worker.node.close();
    let addrs = impostor.transport.addrs();
    tamper_home(&worker, &home.id(), "$.addrs", json!(addrs));
    let r = restart(&net, &worker, "rjw6b", false);
    assert!(r.on);
    stays_apart(&r, &home.id());
    assert!(impostor.node.status()["members"]
        .as_array()
        .is_none_or(|m| m.is_empty()));
}
