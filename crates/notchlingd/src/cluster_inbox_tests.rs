use crate::cluster_inbox_follow::Follow;
use crate::cluster_inbox_testkit::{cluster, node, Fabric, Spec, Wire};
use crate::inbox_mirrors::InboxLink;
use crate::inbox_ops::NO_SUCH_ITEM;
use crate::inbox_ops_tests::{app, item};
use crate::op_group::OpGroup;
use notchling_engine::inbox::{ActionId, State};
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;
use std::sync::Arc;

fn spec(n: u8, client: bool, keeps: bool) -> Spec {
    Spec { n, client, keeps }
}

fn trio() -> Arc<Fabric> {
    cluster(
        &[
            spec(5, true, true),
            spec(1, true, true),
            spec(9, false, false),
        ],
        5,
    )
}

fn settle(fabric: &Fabric) {
    for machine in fabric.machines.lock().unwrap().values() {
        machine.rig.ops.flush();
    }
}

#[test]
fn worker_item_appears_on_home_qualified() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    settle(&fabric);
    let mirrored = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == format!("{}/a", node(9)))
        .expect("the home holds the item");
    assert_eq!(mirrored.source.node, Some(node(9).to_string()));
    assert_eq!(mirrored.state, State::Open);
    assert_eq!(mirrored.title, "title a");
    let events = home.rig.room.named("inbox.changed");
    assert!(events
        .iter()
        .any(|e| e["item"]["id"] == format!("{}/a", node(9))));
    assert_eq!(worker.ids(), vec!["a".to_string()]);
}

#[test]
fn worker_item_appears_on_standby_too() {
    let fabric = trio();
    let (standby, worker) = (fabric.machine(&node(1)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    worker.raise("b");
    worker.outbox.flush();
    assert_eq!(
        standby.ids(),
        vec![format!("{}/a", node(9)), format!("{}/b", node(9))]
    );
    assert_eq!(fabric.machine(&node(5)).ids().len(), 2);
}

#[test]
fn answers_from_home_and_standby_take_one() {
    let fabric = trio();
    let (home, standby, worker) = (
        fabric.machine(&node(5)),
        fabric.machine(&node(1)),
        fabric.machine(&node(9)),
    );
    worker.raise("a");
    worker.outbox.flush();
    let id = format!("{}/a", node(9));
    let (a, b) = std::thread::scope(|scope| {
        let first = scope.spawn(|| home.rig.ops.decide(&id, ActionId::Deny, "mac"));
        let second = scope.spawn(|| standby.rig.ops.decide(&id, ActionId::Allow, "phone"));
        (first.join().unwrap(), second.join().unwrap())
    });
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(outcomes.iter().filter(|o| o.answered).count(), 1);
    assert_eq!(worker.raised.0.lock().unwrap().len(), 1);
    let kept = worker.rig.ops.items().into_iter().next().unwrap();
    assert_eq!(kept.state, State::Answered);
    assert!(outcomes.iter().all(|o| o.state == State::Answered));
}

#[test]
fn answer_on_home_forwards_to_owner_once() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    let id = format!("{}/a", node(9));
    let results = std::thread::scope(|scope| {
        let runs: Vec<_> = (0..2)
            .map(|_| scope.spawn(|| home.rig.ops.decide(&id, ActionId::Allow, "mac")))
            .collect();
        runs.into_iter()
            .map(|run| run.join().unwrap().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|o| o.answered).count(), 1);
    assert_eq!(
        worker.raised.0.lock().unwrap().as_slice(),
        &[("a".to_string(), ActionId::Allow)]
    );
    let late = home.rig.ops.decide(&id, ActionId::Allow, "mac").unwrap();
    assert!(!late.answered);
    assert_eq!(worker.raised.0.lock().unwrap().len(), 1);
}

#[test]
fn the_mirror_changes_when_the_owner_says_so() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    let id = format!("{}/a", node(9));
    home.rig.ops.decide(&id, ActionId::Deny, "mac").unwrap();
    let still = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap();
    assert_eq!(still.state, State::Open);
    worker.outbox.flush();
    let now = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap();
    assert_eq!(now.state, State::Answered);
    assert_eq!(now.answer, Some(ActionId::Deny));
    assert_eq!(now.answered_by.as_deref(), Some("mac"));
}

#[test]
fn owner_offline_answer_fails_plainly() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    fabric.set_down(&node(9), true);
    let id = format!("{}/a", node(9));
    let err = home
        .rig
        .ops
        .decide(&id, ActionId::Allow, "mac")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(err.message.contains("m9 is offline"), "{}", err.message);
    assert_ne!(err.message, NO_SUCH_ITEM);
    assert!(worker.raised.0.lock().unwrap().is_empty());
    let kept = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == id)
        .unwrap();
    assert_eq!(kept.state, State::Open);
}

#[test]
fn reconnect_resends_open_items() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.raise("b");
    worker.outbox.flush();
    assert_eq!(home.ids().len(), 2);
    fabric.set_down(&node(5), true);
    worker.rig.ops.resolve("a", State::Withdrawn, None, None);
    worker.raise("c");
    worker.outbox.flush();
    fabric.set_down(&node(5), false);
    fabric.pass(10_000);
    worker.outbox.flush();
    let open: Vec<String> = home
        .rig
        .ops
        .items()
        .into_iter()
        .filter(|i| i.state == State::Open)
        .map(|i| i.id)
        .collect();
    let mut want = vec![format!("{}/b", node(9)), format!("{}/c", node(9))];
    want.sort();
    let mut open = open;
    open.sort();
    assert_eq!(open, want);
}

#[test]
fn a_restarted_home_is_filled_again() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    home.rig.ops.clear_mirrors();
    assert!(home.ids().is_empty());
    fabric.pass(61_000);
    worker.outbox.flush();
    assert_eq!(home.ids(), vec![format!("{}/a", node(9))]);
}

#[test]
fn a_failed_send_holds_off_then_resyncs() {
    let fabric = trio();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    fabric.set_failing(&node(5), true);
    worker.raise("b");
    worker.outbox.flush();
    fabric.set_failing(&node(5), false);
    worker.outbox.flush();
    assert_eq!(home.ids().len(), 1);
    fabric.pass(5_001);
    worker.outbox.flush();
    assert_eq!(home.ids().len(), 2);
}

#[test]
fn nothing_goes_to_a_machine_that_keeps_nothing() {
    let fabric = cluster(
        &[
            spec(5, true, true),
            spec(9, false, false),
            spec(8, false, false),
        ],
        5,
    );
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    worker.outbox.flush();
    assert!(fabric.sent_to(&node(8)).is_empty());
    assert!(!fabric.sent_to(&node(5)).is_empty());
}

fn client_cluster() -> Arc<Fabric> {
    cluster(
        &[
            spec(5, true, true),
            spec(2, true, false),
            spec(9, false, false),
        ],
        5,
    )
}

#[test]
fn client_lists_home_items_when_linked_else_its_own() {
    let fabric = client_cluster();
    let (home, client, worker) = (
        fabric.machine(&node(5)),
        fabric.machine(&node(2)),
        fabric.machine(&node(9)),
    );
    worker.raise("w1");
    worker.outbox.flush();
    home.raise("h1");
    client.raise("x1");
    client.outbox.flush();
    let listed = |machine: &crate::cluster_inbox_testkit::Machine| {
        let out = machine
            .rig
            .ops
            .call(&app(), ops::INBOX_LIST, &json!({}))
            .unwrap();
        let mut ids: Vec<String> = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    let mut want = vec![
        "x1".to_string(),
        format!("{}/h1", node(5)),
        format!("{}/w1", node(9)),
    ];
    want.sort();
    assert_eq!(listed(&client), want);
    assert!(home.ids().contains(&format!("{}/x1", node(2))));
    fabric.set_down(&node(5), true);
    assert_eq!(listed(&client), vec!["x1".to_string()]);
}

#[test]
fn a_client_answers_a_home_item_through_the_home_and_its_own_directly() {
    let fabric = client_cluster();
    let (home, client) = (fabric.machine(&node(5)), fabric.machine(&node(2)));
    home.raise("h1");
    client.raise("x1");
    client.outbox.flush();
    let through = client
        .rig
        .ops
        .decide(&format!("{}/h1", node(5)), ActionId::Allow, "phone")
        .unwrap();
    assert!(through.answered);
    assert_eq!(
        home.raised.0.lock().unwrap().as_slice(),
        &[("h1".to_string(), ActionId::Allow)]
    );
    let home_item = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == "h1")
        .unwrap();
    assert_eq!(home_item.answered_by.as_deref(), Some("phone"));
    let own = client
        .rig
        .ops
        .decide(&format!("{}/x1", node(2)), ActionId::Deny, "mac")
        .unwrap();
    assert!(own.answered);
    assert_eq!(client.raised.0.lock().unwrap().len(), 1);
}

#[test]
fn the_followed_home_list_is_pushed_to_the_clients_app() {
    let fabric = client_cluster();
    let (home, client) = (fabric.machine(&node(5)), fabric.machine(&node(2)));
    let wire = Arc::new(Wire {
        me: node(2),
        fabric: fabric.clone(),
    });
    let follow = Follow::new(
        client.rig.ops.clone(),
        Arc::new(crate::cluster_inbox::NetLink::new(wire)),
    );
    home.raise("h1");
    follow.poll(0);
    client.rig.ops.flush();
    let id = format!("{}/h1", node(5));
    assert!(client
        .rig
        .room
        .named("inbox.changed")
        .iter()
        .any(|e| e["item"]["id"] == id));
    home.rig.ops.decide("h1", ActionId::Deny, "mac").unwrap();
    follow.poll(100);
    follow.poll(5_000);
    client.rig.ops.flush();
    let last = client.rig.room.named("inbox.changed").pop().unwrap();
    assert_eq!(last["item"]["state"], "answered");
    fabric.set_down(&node(5), true);
    follow.poll(10_000);
    client.rig.ops.flush();
    assert!(client
        .rig
        .room
        .named("inbox.removed")
        .iter()
        .any(|e| e["id"] == id));
}

struct Stuck {
    gate: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl InboxLink for Stuck {
    fn me(&self) -> Option<notchling_engine::cluster::NodeId> {
        Some(node(1))
    }

    fn answer(
        &self,
        _owner: &notchling_engine::cluster::NodeId,
        _id: &str,
        _action: ActionId,
        _by: &str,
    ) -> Result<serde_json::Value, notchling_local_api::ApiError> {
        let _ = self.gate.lock().unwrap().recv();
        Err(notchling_local_api::ApiError::new(
            ErrorCode::Failed,
            "the cluster node didn't answer",
        ))
    }

    fn home_items(&self) -> crate::inbox_mirrors::Home {
        crate::inbox_mirrors::Home::Off
    }
}

#[test]
fn decide_holds_no_lock_while_forwarding() {
    let r = crate::inbox_ops_tests::rig();
    let (release, gate) = std::sync::mpsc::channel();
    r.ops.set_link(Arc::new(Stuck {
        gate: std::sync::Mutex::new(gate),
    }));
    r.ops.mirror_changed(&node(7), item("a")).unwrap();
    let id = format!("{}/a", node(7));
    let ops = r.ops.clone();
    let blocked = std::thread::spawn(move || ops.decide(&id, ActionId::Deny, "mac"));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let listing = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(listing["items"].as_array().unwrap().len(), 1);
    r.ops.raise(item("own"), None);
    assert_eq!(r.ops.items().len(), 2);
    release.send(()).unwrap();
    let err = blocked.join().unwrap().unwrap_err();
    assert_eq!(err.message, "the cluster node didn't answer");
    let mirror = r
        .ops
        .items()
        .into_iter()
        .find(|i| i.id.contains('/'))
        .unwrap();
    assert_eq!(mirror.state, State::Open);
}

struct Recording {
    calls: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl crate::cluster_inbox::Net for Recording {
    fn me(&self) -> Option<notchling_engine::cluster::NodeId> {
        Some(node(1))
    }

    fn home(&self) -> Option<notchling_engine::cluster::NodeId> {
        None
    }

    fn peers(&self) -> Vec<crate::cluster_inbox::Peer> {
        vec![crate::cluster_inbox::Peer {
            id: node(9),
            name: "worker".to_string(),
            online: true,
            keeps: false,
            consent: true,
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
        _to: &notchling_engine::cluster::NodeId,
        _op: &str,
        args: serde_json::Value,
        _tier: notchling_engine::cluster::wire::OriginTier,
        _within: std::time::Duration,
    ) -> Result<serde_json::Value, notchling_local_api::ApiError> {
        self.calls.lock().unwrap().push(args);
        Ok(json!({"answered": true}))
    }
}

#[test]
fn a_forwarded_answer_carries_the_note_only_when_there_is_one() {
    let net = Arc::new(Recording {
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let link = crate::cluster_inbox::NetLink::new(net.clone());
    link.answer_noted(&node(9), "a", ActionId::Deny, "mac", Some("rename it"))
        .unwrap();
    link.answer(&node(9), "b", ActionId::Deny, "mac").unwrap();
    let calls = net.calls.lock().unwrap();
    assert_eq!(calls[0]["note"], "rename it");
    assert_eq!(calls[0]["action"], "deny");
    assert!(calls[1].get("note").is_none());
}
