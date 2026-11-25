use super::{Net, Outbox, Peer};
use crate::cluster_inbox_follow::Follow;
use crate::cluster_inbox_testkit::{cluster, node, Fabric, Spec, Wire};
use crate::cluster_node::LocalCalls;
use crate::inbox_ops_tests::{item, rig, NOW};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{ActionId, Change, State};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn spec(n: u8, client: bool, keeps: bool) -> Spec {
    Spec { n, client, keeps }
}

fn quartet() -> Arc<Fabric> {
    cluster(
        &[
            spec(5, true, true),
            spec(2, true, true),
            spec(9, false, false),
        ],
        5,
    )
}

#[test]
fn an_owner_removed_from_the_cluster_loses_its_mirrors() {
    let fabric = quartet();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    assert_eq!(home.ids().len(), 1);
    fabric.remove(&node(9));
    home.outbox.flush();
    home.rig.ops.flush();
    assert!(home.ids().is_empty());
    assert!(home
        .rig
        .room
        .named("inbox.removed")
        .iter()
        .any(|e| e["id"] == format!("{}/a", node(9))));
}

#[test]
fn a_node_that_stops_keeping_drops_every_mirror() {
    let fabric = quartet();
    let (standby, worker) = (fabric.machine(&node(2)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    assert_eq!(standby.rig.ops.local_ids().len(), 1);
    fabric.stop_keeping(&node(2));
    standby.outbox.flush();
    standby.outbox.flush();
    assert!(standby.rig.ops.local_ids().is_empty());
}

#[test]
fn a_home_item_that_becomes_a_local_mirror_is_not_removed() {
    let fabric = quartet();
    let (standby, worker) = (fabric.machine(&node(2)), fabric.machine(&node(9)));
    let wire = Arc::new(Wire {
        me: node(2),
        fabric: fabric.clone(),
    });
    let follow = Follow::new(
        standby.rig.ops.clone(),
        Arc::new(crate::cluster_inbox::NetLink::new(wire)),
    );
    fabric.set_failing(&node(2), true);
    worker.raise("x");
    worker.outbox.flush();
    follow.poll(0);
    let id = format!("{}/x", node(9));
    assert!(standby.ids().contains(&id));
    fabric.set_failing(&node(2), false);
    fabric.pass(5_001);
    worker.outbox.flush();
    follow.poll(10_000);
    standby.rig.ops.flush();
    assert!(standby
        .rig
        .room
        .named("inbox.removed")
        .iter()
        .all(|e| e["id"] != id));
    assert!(standby.ids().contains(&id));
}

#[test]
fn a_failed_home_list_keeps_what_was_shown() {
    let fabric = quartet();
    let (home, standby) = (fabric.machine(&node(5)), fabric.machine(&node(2)));
    let wire = Arc::new(Wire {
        me: node(2),
        fabric: fabric.clone(),
    });
    let follow = Follow::new(
        standby.rig.ops.clone(),
        Arc::new(crate::cluster_inbox::NetLink::new(wire)),
    );
    home.raise("h1");
    follow.poll(0);
    fabric.set_failing(&node(5), true);
    follow.poll(3_000);
    fabric.set_failing(&node(5), false);
    follow.poll(6_000);
    standby.rig.ops.flush();
    assert!(standby.rig.room.named("inbox.removed").is_empty());
    assert_eq!(standby.rig.room.named("inbox.changed").len(), 1);
}

#[test]
fn a_mirror_stays_open_until_its_owner_says_otherwise() {
    let fabric = quartet();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let mut made = item("a");
    made.expires_ms = Some(NOW + 1_000);
    worker.rig.ops.raise(made, Some(worker.raised.clone()));
    worker.outbox.flush();
    home.rig.ops.tick(NOW + 10_000_000);
    let id = format!("{}/a", node(9));
    let shown = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap();
    assert_eq!(shown.state, State::Open);
    let out = home.rig.ops.decide(&id, ActionId::Allow, "mac").unwrap();
    assert!(out.answered);
}

#[test]
fn a_refusing_peer_is_asked_less_and_less_often() {
    let fabric = quartet();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    fabric.set_failing(&node(5), true);
    let home = node(5);
    for _ in 0..6 {
        worker.outbox.flush();
        fabric.pass(5_001);
    }
    assert_eq!(fabric.attempts_to(&home), 3);
    fabric.set_failing(&node(5), false);
    fabric.pass(300_000);
    worker.outbox.flush();
    assert_eq!(fabric.machine(&node(5)).ids().len(), 1);
    fabric.set_failing(&node(5), true);
    fabric.pass(61_000);
    let before = fabric.attempts_to(&home);
    worker.outbox.flush();
    fabric.pass(5_001);
    worker.outbox.flush();
    assert_eq!(fabric.attempts_to(&home), before + 2);
}

#[test]
fn a_change_in_who_is_asked_ends_the_wait() {
    let fabric = quartet();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    fabric.set_failing(&node(5), true);
    fabric.set_failing(&node(2), true);
    worker.outbox.flush();
    let before = fabric.attempts_to(&node(2));
    worker.outbox.flush();
    assert_eq!(fabric.attempts_to(&node(2)), before);
    fabric.set_down(&node(5), true);
    worker.outbox.flush();
    assert_eq!(fabric.attempts_to(&node(2)), before + 1);
}

#[test]
fn what_a_mirror_holds_is_bounded() {
    let fabric = quartet();
    let home = fabric.machine(&node(5));
    let sender = Origin {
        node: node(9),
        tier: OriginTier::Node,
    };
    let mut big = item("a");
    big.actions = (0..40)
        .map(|n| notchling_engine::inbox::Action {
            id: ActionId::Allow,
            label: "L".repeat(500 + n),
            primary: false,
        })
        .collect();
    big.source.job = Some("j".repeat(5_000));
    big.source.session = Some("s".repeat(5_000));
    big.answered_by = Some("b".repeat(5_000));
    home.calls
        .call(
            "node.inbox",
            &json!({"event": "changed", "item": big}),
            &sender,
        )
        .unwrap();
    let shown = home.rig.ops.items().into_iter().next().unwrap();
    assert_eq!(shown.actions.len(), 8);
    assert!(shown.actions.iter().all(|a| a.label.chars().count() <= 60));
    assert!(shown.source.job.unwrap().chars().count() <= 200);
    assert!(shown.source.session.unwrap().chars().count() <= 200);
    assert!(shown.answered_by.unwrap().chars().count() <= 40);
    let long = item(&"i".repeat(500));
    let err = home
        .calls
        .call(
            "node.inbox",
            &json!({"event": "changed", "item": long}),
            &sender,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert_eq!(home.ids().len(), 1);
}

struct Blocked {
    gate: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Net for Blocked {
    fn me(&self) -> Option<NodeId> {
        Some(node(1))
    }

    fn home(&self) -> Option<NodeId> {
        Some(node(5))
    }

    fn peers(&self) -> Vec<Peer> {
        vec![Peer {
            id: node(5),
            name: "m5".to_string(),
            online: true,
            keeps: true,
            consent: false,
        }]
    }

    fn is_client(&self) -> bool {
        false
    }

    fn keeps(&self) -> bool {
        false
    }

    fn call(
        &self,
        _to: &NodeId,
        _op: &str,
        _args: Value,
        _tier: OriginTier,
        _within: Duration,
    ) -> Result<Value, ApiError> {
        let _ = self.gate.lock().unwrap().recv();
        Err(ApiError::new(ErrorCode::Failed, "gone"))
    }
}

#[test]
fn close_does_not_wait_on_a_blocked_call() {
    let r = rig();
    let (release, gate) = std::sync::mpsc::channel();
    let net = Arc::new(Blocked {
        gate: std::sync::Mutex::new(gate),
    });
    let outbox = Outbox::new(net, r.ops.clone(), Arc::new(|| 0));
    outbox.push(vec![Change::Raised(item("a"))]);
    outbox.spawn();
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    outbox.close();
    assert!(started.elapsed() < Duration::from_secs(3));
    let _ = release.send(());
}

#[test]
fn the_list_keeps_the_homes_items_while_the_home_is_slow_and_drops_them_when_it_is_gone() {
    let fabric = quartet();
    let (home, standby) = (fabric.machine(&node(5)), fabric.machine(&node(2)));
    home.raise("h1");
    let id = format!("{}/h1", node(5));
    let listed = || standby.rig.ops.items().into_iter().any(|i| i.id == id);
    assert!(listed());
    fabric.set_failing(&node(5), true);
    assert!(listed());
    fabric.set_failing(&node(5), false);
    assert!(listed());
    fabric.set_down(&node(5), true);
    assert!(!listed());
}

#[test]
fn one_flicker_of_the_keeping_role_keeps_the_mirrors() {
    let fabric = quartet();
    let (standby, worker) = (fabric.machine(&node(2)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    fabric.stop_keeping(&node(2));
    standby.outbox.flush();
    assert_eq!(standby.rig.ops.local_ids().len(), 1);
    fabric.resume_keeping(&node(2));
    standby.outbox.flush();
    fabric.stop_keeping(&node(2));
    standby.outbox.flush();
    assert_eq!(standby.rig.ops.local_ids().len(), 1);
    standby.outbox.flush();
    assert!(standby.rig.ops.local_ids().is_empty());
}

fn consent_item(id: &str) -> notchling_engine::inbox::Item {
    let mut made = item(id);
    made.kind = notchling_engine::inbox::Kind::Consent;
    made
}

fn kinds_sent(fabric: &Fabric, to: &NodeId) -> Vec<String> {
    fabric
        .sent_to(to)
        .into_iter()
        .filter(|(_, op, _)| op == "node.inbox")
        .flat_map(|(_, _, args)| {
            let one = args.get("item").cloned().into_iter();
            let many = args
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            one.chain(many)
                .filter_map(|i| i["kind"].as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_consent_item_reaches_an_older_peer_as_a_notice_in_a_sync_and_in_a_change() {
    let fabric = quartet();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker
        .rig
        .ops
        .raise(consent_item("c"), Some(worker.raised.clone()));
    worker.outbox.flush();
    let kinds = kinds_sent(&fabric, &node(5));
    assert!(!kinds.is_empty());
    assert!(kinds.iter().all(|k| k == "notice"), "{kinds:?}");
    worker
        .rig
        .ops
        .raise(consent_item("d"), Some(worker.raised.clone()));
    worker.outbox.flush();
    let kinds = kinds_sent(&fabric, &node(5));
    assert!(kinds.len() >= 2);
    assert!(kinds.iter().all(|k| k == "notice"), "{kinds:?}");
    let mirrored = home
        .rig
        .ops
        .items()
        .into_iter()
        .filter(|i| i.id.ends_with("/c") || i.id.ends_with("/d"))
        .count();
    assert_eq!(mirrored, 2);
    assert!(worker
        .rig
        .ops
        .items()
        .iter()
        .all(|i| i.kind == notchling_engine::inbox::Kind::Consent));
}

#[test]
fn a_consent_item_reaches_a_peer_that_knows_the_kind_as_consent() {
    let fabric = quartet();
    fabric.knows_consent.lock().unwrap().insert(node(5));
    let worker = fabric.machine(&node(9));
    worker
        .rig
        .ops
        .raise(consent_item("c"), Some(worker.raised.clone()));
    worker.outbox.flush();
    worker
        .rig
        .ops
        .raise(consent_item("d"), Some(worker.raised.clone()));
    worker.outbox.flush();
    let kinds = kinds_sent(&fabric, &node(5));
    assert!(kinds.len() >= 2, "{kinds:?}");
    assert!(kinds.iter().all(|k| k == "consent"), "{kinds:?}");
}

#[test]
fn a_member_knows_the_consent_kind_only_when_its_versions_say_so() {
    use notchling_engine::cluster::versions::FEATURE_INBOX_CONSENT;
    let knows = json!({"versions": {"features": ["dispatch", FEATURE_INBOX_CONSENT]}});
    assert!(crate::cluster_inbox::knows_consent(&knows));
    for older in [
        json!({"versions": {"features": ["dispatch"]}}),
        json!({"versions": {}}),
        json!({}),
    ] {
        assert!(!crate::cluster_inbox::knows_consent(&older), "{older}");
    }
}
