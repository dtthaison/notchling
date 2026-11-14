use super::{HeadlessSwitches, NodeCalls, SwitchPanel, NODE_SWITCH_SET};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{Hooks, LocalCalls};
use crate::cluster_testkit::{eventually, pair, Dir, FakeCalls, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{Desired, LocalCmd};
use notchling_local_api::{ApiError, AuditLog, ErrorCode};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Panel {
    on: Mutex<BTreeMap<String, bool>>,
    sets: Mutex<Vec<(String, bool)>>,
}

impl SwitchPanel for Panel {
    fn get(&self, switch: &str) -> Option<bool> {
        Some(*self.on.lock().unwrap().get(switch).unwrap_or(&false))
    }

    fn set(&self, switch: &str, on: bool) -> Result<(), ApiError> {
        self.on.lock().unwrap().insert(switch.to_string(), on);
        self.sets.lock().unwrap().push((switch.to_string(), on));
        Ok(())
    }
}

struct Kit {
    panel: Arc<Panel>,
    room: Arc<FakeRoom>,
    switches: Arc<HeadlessSwitches>,
    dir: Dir,
}

impl Kit {
    fn audit(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("audit.jsonl")).unwrap_or_default()
    }
}

fn kit(tag: &str) -> Kit {
    let dir = Dir::new(tag);
    let panel = Arc::new(Panel::default());
    let room = Arc::new(FakeRoom::default());
    let switches = Arc::new(HeadlessSwitches::new(
        panel.clone(),
        room.clone(),
        AuditLog::new(dir.path().join("audit.jsonl")),
    ));
    Kit {
        panel,
        room,
        switches,
        dir,
    }
}

struct Follows(Arc<HeadlessSwitches>);

impl Hooks for Follows {
    fn desired(&self, desired: Desired) {
        self.0.follow(desired);
    }
}

#[test]
fn home_desired_turns_acting_on_headless_node() {
    let k = kit("desired");
    k.switches.follow(Desired {
        acting: true,
        bridge: false,
        install: true,
    });
    let sets = k.panel.sets.lock().unwrap().clone();
    assert_eq!(
        sets,
        vec![("acting".to_string(), true), ("install".to_string(), true)]
    );
}

#[test]
fn desired_never_touches_vault_or_accounts() {
    let k = kit("never");
    for switch in ["vault", "accounts", "cluster", "anything"] {
        let err = k.switches.set(switch, true, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{switch}");
    }
    assert!(k.panel.sets.lock().unwrap().is_empty());
    assert!(k.audit().is_empty());
}

#[test]
fn node_with_app_ignores_home_desired() {
    let k = kit("withapp");
    k.room.app.store(true, Ordering::SeqCst);
    k.switches.follow(Desired {
        acting: true,
        bridge: true,
        install: true,
    });
    let answer = k.switches.set("acting", true, None).unwrap();
    assert_eq!(answer["applied"], false);
    assert!(k.panel.sets.lock().unwrap().is_empty());
}

#[test]
fn switch_changes_are_audited() {
    let k = kit("audited");
    k.switches
        .set("bridge", true, Some("0123456789abcdef"))
        .unwrap();
    let again = k.switches.set("bridge", true, None).unwrap();
    assert_eq!(again["applied"], false);
    k.switches.set("bridge", false, None).unwrap();
    let audit = k.audit();
    let lines: Vec<&str> = audit.lines().collect();
    assert_eq!(lines.len(), 2, "{audit}");
    assert!(lines[0].contains("cluster_set:bridge:on"), "{audit}");
    assert!(lines[0].contains("0123456789abcdef"), "{audit}");
    assert!(lines[1].contains("cluster_set:bridge:off"), "{audit}");
}

#[test]
fn node_switch_set_from_the_home_reaches_a_headless_worker() {
    let net = MemNet::new();
    let k = kit("nodeset");
    let mut worker = Opts::named("sworker1");
    worker.local = Some(Arc::new(NodeCalls {
        inner: Arc::new(FakeCalls::default()),
        switches: k.switches.clone(),
    }));
    let (home, w) = pair(&net, Opts::named("shome1"), worker);
    let origin = Origin {
        node: home.id(),
        tier: OriginTier::Node,
    };
    let answer = home
        .node
        .local(LocalCmd::Forward {
            node: w.id(),
            op: NODE_SWITCH_SET.to_string(),
            args: json!({"switch": "acting", "on": true}),
            origin: origin.clone(),
        })
        .unwrap();
    assert_eq!(answer["applied"], true, "{answer}");
    assert_eq!(k.panel.get("acting"), Some(true));
    let refused = home
        .node
        .local(LocalCmd::Forward {
            node: w.id(),
            op: NODE_SWITCH_SET.to_string(),
            args: json!({"switch": "vault", "on": true}),
            origin,
        })
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden, "{refused:?}");
    assert_eq!(k.panel.get("vault"), Some(false));
    assert!(k.audit().contains("cluster_set:acting:on"));
}

#[test]
fn a_worker_follows_what_the_home_stores_for_it() {
    let net = MemNet::new();
    let k = kit("follow");
    let mut worker = Opts::named("sworker2");
    worker.hooks = Some(Arc::new(Follows(k.switches.clone())));
    let (home, w) = pair(&net, Opts::named("shome2"), worker);
    home.node
        .local(LocalCmd::SetMember {
            node: w.id(),
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
    eventually("acting follows the home", || {
        k.panel.get("acting") == Some(true)
    });
    assert_eq!(k.panel.get("vault"), Some(false));
}

#[test]
fn other_ops_pass_through() {
    let k = kit("pass");
    let inner = Arc::new(FakeCalls::default());
    let calls = NodeCalls {
        inner: inner.clone(),
        switches: k.switches.clone(),
    };
    let origin = Origin {
        node: notchling_engine::cluster::NodeId::parse("0123456789abcdef").unwrap(),
        tier: OriginTier::Node,
    };
    calls.call("shell.run", &json!({}), &origin).unwrap();
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    let bad = calls
        .call(NODE_SWITCH_SET, &json!({"switch": "acting"}), &origin)
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::BadRequest);
}
