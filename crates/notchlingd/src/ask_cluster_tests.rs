use crate::acting_ops_tests::app;
use crate::ask_testkit::{open_with, rig_on, wait_as, Rig, ASKER};
use crate::ask_wiring::{worker_in, worker_name, Slot};
use crate::cluster_inbox_testkit::{cluster, node, Fabric, Machine, Spec};
use crate::cluster_move_tests::member;
use crate::op_group::OpGroup;
use notchling_engine::ask::AskState;
use notchling_engine::ask::By;
use notchling_engine::cluster::state::{start, Command};
use notchling_engine::cluster::ClusterState;
use notchling_engine::inbox::{ActionId, Kind, State};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;

const REFUSAL: &str =
    "On a Worker, give choices (--choice): the answer comes from your Mac's Inbox.";

fn state() -> ClusterState {
    let mut home = member(&node(5), "m5");
    home.client = false;
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "work".to_string(),
            home,
        },
        1,
    )
    .expect("create");
    for (n, name, client, can) in [
        (1, "m1", false, true),
        (9, "m9", false, false),
        (7, "m7", true, false),
    ] {
        let mut made = member(&node(n), name);
        made.client = client;
        made.can_be_master = can;
        state.members.insert(node(n), made);
    }
    state
}

fn worker_rig(machine: &Machine) -> Rig {
    rig_on(machine.rig.ops.clone(), Arc::new(|| Some("m9".to_string())))
}

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

fn settle(fabric: &Fabric) {
    for machine in fabric.machines.lock().unwrap().values() {
        machine.rig.ops.flush();
    }
}

#[test]
fn worker_refuses_a_question_without_choices() {
    let fabric = pair();
    let worker = worker_rig(&fabric.machine(&node(9)));
    let err = open_with(&worker, json!({"question": "Which?", "key": "k1"})).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert_eq!(err.message, REFUSAL);
    assert!(worker.store.list(None).unwrap().is_empty());
    assert!(fabric.machine(&node(9)).ids().is_empty());
}

#[test]
fn worker_question_answered_from_home() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let worker = worker_rig(&machine);
    let opened = open_with(
        &worker,
        json!({"question": "Which?", "choices": ["a", "b", "c"], "key": "k1"}),
    )
    .unwrap();
    let id = opened["id"].as_str().unwrap().to_string();
    let inbox_id = worker.store.get(&id).unwrap().unwrap().inbox_id.unwrap();
    machine.outbox.flush();
    settle(&fabric);
    let qualified = format!("{}/{}", node(9), inbox_id);
    let listed = home
        .rig
        .ops
        .call(&app(), ops::INBOX_LIST, &json!({}))
        .unwrap();
    let shown: Value = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == json!(qualified))
        .cloned()
        .expect("the home lists the question");
    assert_eq!(shown["kind"], json!("question"));
    assert!(shown["title"].as_str().unwrap().ends_with(" on m9"));
    assert_eq!(shown["source"]["node"], json!(node(9).to_string()));
    assert_eq!(shown["detail"], json!("Which?"));
    let done = home
        .rig
        .ops
        .decide(&qualified, ActionId::Choice2, "phone")
        .unwrap();
    assert!(done.answered);
    worker.ops.drain();
    let row = worker.store.get(&id).unwrap().unwrap();
    assert_eq!(row.state, AskState::Answered);
    assert_eq!(row.choice, Some(2));
    assert_eq!(row.by, Some(By::Phone));
    let reply = wait_as(&worker, ASKER, &id, 500).unwrap();
    assert_eq!(reply["choice"], json!(2));
    assert_eq!(reply["answer"], json!("b"));
    assert_eq!(reply["by"], json!("phone"));
    settle(&fabric);
    let kept = machine
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == inbox_id)
        .unwrap();
    assert_eq!(kept.kind, Kind::Question);
    assert_eq!(kept.state, State::Answered);
    assert_eq!(kept.answered_by.as_deref(), Some("phone"));
}

#[test]
fn standby_and_lone_node_take_questions_without_choices() {
    let fabric = pair();
    let all = state();
    let standby = node(1);
    let named = {
        let all = all.clone();
        Arc::new(move || worker_name(&all, &standby)) as crate::ask_ops::Worker
    };
    let lone = worker_in(Slot::default());
    for (tag, worker) in [("standby", named), ("lone", lone)] {
        let rig = rig_on(fabric.machine(&node(9)).rig.ops.clone(), worker);
        let opened = open_with(&rig, json!({"question": "Which?", "key": tag}));
        let id = opened.unwrap()["id"].as_str().unwrap().to_string();
        let row = rig.store.get(&id).unwrap().unwrap();
        assert_eq!(row.choices, None, "{tag}");
        let item = rig
            .inbox
            .items()
            .into_iter()
            .find(|i| Some(&i.id) == row.inbox_id.as_ref())
            .unwrap();
        assert!(!item.title.contains(" on "), "{tag}");
        assert_eq!(item.source.node, None, "{tag}");
    }
}

#[test]
fn only_a_member_that_is_no_home_standby_or_client_is_a_worker() {
    let all = state();
    assert_eq!(worker_name(&all, &node(9)), Some("m9".to_string()));
    assert_eq!(worker_name(&all, &node(5)), None);
    assert_eq!(worker_name(&all, &node(1)), None);
    assert_eq!(worker_name(&all, &node(7)), None);
    assert_eq!(worker_name(&all, &node(3)), None);
}
