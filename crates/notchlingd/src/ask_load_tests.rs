use crate::ask_testkit::{
    base, new_row, open, open_with, person, rig, rig_with, scratch, state_of, wait_as, ASKER, T0,
};
use crate::event_assert::{of, text};
use notchling_engine::ask::{AskState, AskStore};
use notchling_local_api::ops;
use serde_json::json;
use std::sync::Arc;

fn restored(r: &crate::ask_testkit::Rig) -> usize {
    of(&r.sink, "session.ask")
        .iter()
        .filter(|e| text(e, "then").as_deref() == Some("restored"))
        .count()
}

#[test]
fn load_keeps_open_and_answered_rows() {
    let data = scratch("load-data");
    let first = rig_with(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("load-a"),
        )),
    );
    let open_id = open(&first);
    let mut args = base();
    args["key"] = json!("k2");
    let answered_id = open_with(&first, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut args = base();
    args["key"] = json!("k3");
    let done_id = open_with(&first, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    person(
        &first,
        ops::ASK_ANSWER,
        json!({"id": answered_id, "choice": 2}),
    )
    .unwrap();
    person(&first, ops::ASK_DISMISS, json!({"id": done_id})).unwrap();
    drop(first);

    let second = rig_with(
        json!([]),
        Some((
            Arc::new(AskStore::open(&data.0).unwrap()),
            scratch("load-b"),
        )),
    );
    second.ops.load(T0 + 5_000);
    assert_eq!(restored(&second), 2);
    assert_eq!(state_of(&second, &open_id), "open");
    assert_eq!(state_of(&second, &answered_id), "answered");
    assert_eq!(state_of(&second, &done_id), "dismissed");
    let reply = wait_as(&second, ASKER, &answered_id, 1_000).unwrap();
    assert_eq!(reply["state"], json!("answered"));
    assert_eq!(reply["answer"], json!("b"));
    assert_eq!(state_of(&second, &answered_id), "delivered");
}

#[test]
fn answer_given_with_no_waiter_is_handed_over_next_wait() {
    let r = rig();
    let id = open(&r);
    person(&r, ops::ASK_ANSWER, json!({"id": id, "text": "go ahead"})).unwrap();
    r.ops.tick(T0 + 1_000);
    r.ops.tick(T0 + 2_000);
    assert_eq!(state_of(&r, &id), "answered");
    let reply = wait_as(&r, ASKER, &id, 1_000).unwrap();
    assert_eq!(reply["answer"], json!("go ahead"));
    assert!(reply.get("choice").is_none());
    assert_eq!(state_of(&r, &id), "delivered");
}

#[test]
fn load_skips_closed_rows() {
    let r = rig();
    let row = r.store.insert(&new_row("s", "k", ASKER), T0).unwrap();
    r.store
        .close(&row.id, &[AskState::Open], AskState::Dismissed, T0)
        .unwrap();
    r.ops.load(T0 + 1);
    assert_eq!(restored(&r), 0);
}
