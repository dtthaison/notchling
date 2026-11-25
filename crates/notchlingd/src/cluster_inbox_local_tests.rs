use crate::cluster_inbox_testkit::{cluster, node, Fabric, Spec};
use crate::inbox_mirrors::{display, offered};
use crate::inbox_ops::NO_SUCH_ITEM;
use crate::inbox_ops_tests::{app, caller, item};
use crate::op_group::OpGroup;
use notchling_engine::bridge::inbox::wire_item;
use notchling_engine::inbox::{Action, ActionId, Change, Item, Kind, SHELL_STUCK_ID_PREFIX};
use notchling_local_api::{ops, ClientKind, Tier};
use serde_json::json;
use std::sync::Arc;

fn pair() -> Arc<Fabric> {
    cluster(
        &[
            Spec {
                n: 5,
                client: true,
                keeps: true,
            },
            Spec {
                n: 9,
                client: false,
                keeps: false,
            },
        ],
        5,
    )
}

fn stuck(tail: &str) -> Item {
    let mut made = item(&format!("{SHELL_STUCK_ID_PREFIX}{tail}"));
    made.kind = Kind::ShellStuck;
    made.actions = vec![
        Action {
            id: ActionId::Stop,
            label: "Stop".to_string(),
            primary: true,
        },
        Action {
            id: ActionId::KeepWaiting,
            label: "Keep Waiting".to_string(),
            primary: false,
        },
    ];
    made
}

fn nothing_stuck_was_sent(fabric: &Fabric) -> bool {
    fabric
        .sent_to(&node(5))
        .iter()
        .all(|(_, _, args)| !args.to_string().contains(SHELL_STUCK_ID_PREFIX))
}

#[test]
fn a_stuck_shell_never_goes_to_a_peer_by_event_sync_or_list() {
    let fabric = pair();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.rig.ops.raise(item("plain"), None);
    worker.outbox.flush();
    let made = stuck("s1:1:100");
    worker.rig.ops.raise(made.clone(), None);
    worker.outbox.flush();
    worker.rig.ops.flush();
    assert!(!fabric.sent_to(&node(5)).is_empty());
    assert!(nothing_stuck_was_sent(&fabric));
    assert_eq!(home.ids(), vec![format!("{}/plain", node(9))]);
    assert!(worker.ids().contains(&made.id));

    let as_node = caller(Tier::App, ClientKind::Node);
    let listed = worker
        .rig
        .ops
        .call(&as_node, ops::INBOX_LIST, &json!({}))
        .unwrap();
    let listed_ids: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed_ids, vec!["plain"]);
    let own = worker
        .rig
        .ops
        .call(&app(), ops::INBOX_LIST, &json!({}))
        .unwrap();
    assert_eq!(own["items"].as_array().unwrap().len(), 2);
}

#[test]
fn a_stuck_shell_change_or_removal_keeps_off_the_wire() {
    let fabric = pair();
    let worker = fabric.machine(&node(9));
    worker.outbox.flush();
    worker.outbox.push(vec![
        Change::Raised(stuck("s2:2:200")),
        Change::Updated(stuck("s2:2:200")),
        Change::Removed(format!("{SHELL_STUCK_ID_PREFIX}s2:2:200")),
    ]);
    worker.outbox.flush();
    assert!(nothing_stuck_was_sent(&fabric));
}

#[test]
fn a_stuck_shell_is_not_shown_to_the_phone_and_the_phone_cannot_answer_it() {
    let fabric = pair();
    let worker = fabric.machine(&node(9));
    let made = stuck("s1:1:100");
    worker.rig.ops.raise(made.clone(), None);
    worker.rig.ops.raise(item("plain"), None);
    let phone: Vec<String> = worker
        .rig
        .ops
        .phone_items()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(phone, vec!["plain".to_string()]);
    let err = worker
        .rig
        .ops
        .decide_for_phone(&made.id, ActionId::Stop, "phone")
        .unwrap_err();
    assert_eq!(err.message, NO_SUCH_ITEM);
    assert_eq!(wire_item(&made).kind, "shell_stuck");
}

#[test]
fn a_mirror_keeps_no_action_of_a_stuck_shell() {
    assert!(offered(Kind::ShellStuck).is_empty());
    let shown = display(&node(9), stuck("s1:1:100")).unwrap();
    assert!(shown.actions.is_empty());
}
