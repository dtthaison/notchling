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
            id: ActionId::Done,
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
