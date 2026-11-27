use super::*;
use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::outbox::{Verdict, UNSURE_HOLD_MS};
use serde_json::{json, Value};

fn state(r: &Rig, id: &str) -> Option<(State, Option<Why>)> {
    r.row(id).map(|row| (row.state, row.why))
}

fn call(r: &Rig, op: &str, args: Value) -> Result<Value, notchling_local_api::ApiError> {
    r.ops.call(&app(), op, &args)
}

fn unsure_first(r: &Rig, text: &str) -> (String, Postman) {
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        state(r, &row.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    (row.id, p)
}

#[test]
fn a_row_queued_behind_an_unsure_one_goes_once_the_session_has_stayed_idle() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    let second = r.send("job-idle", "add id to the lists");
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 2_000));
    p.pass(r.at(T0 + 3_000));
    assert_eq!(
        state(&r, &second.id),
        Some((State::Queued, None)),
        "a short grace first"
    );
    p.pass(r.at(T0 + 4_000));
    assert_eq!(r.row(&second.id), None, "typed and settled as sent");
    assert_eq!(r.returns(), 2);
    assert!(r.typed().contains("add id to the lists"));
    let kept = r.row(&first).unwrap();
    assert_eq!(
        kept.state,
        State::Unsure,
        "the unsure row stays, with its buttons"
    );
    assert_eq!(kept.aside_ms, Some(T0 + 4_000), "it stepped aside early");
}

#[test]
fn an_unsure_row_with_nothing_behind_it_keeps_its_hold() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 30_000));
    let kept = r.row(&first).unwrap();
    assert_eq!(kept.aside_ms, None);
    assert_eq!(kept.aside_at_ms, Some(T0 + UNSURE_HOLD_MS));
}

#[test]
fn the_unsure_row_keeps_holding_while_the_session_is_in_a_turn() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    r.files.set_status(IDLE, "busy");
    let second = r.send("job-idle", "add id to the lists");
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 30_000));
    assert_eq!(r.returns(), 1, "nothing is typed behind it yet");
    assert_eq!(state(&r, &second.id), Some((State::Queued, None)));
    assert_eq!(r.row(&first).unwrap().aside_ms, None);
    r.files.set_status(IDLE, "idle");
    p.pass(r.at(T0 + 31_000));
    assert_eq!(r.returns(), 1, "one reading of idle is not enough");
    p.pass(r.at(T0 + 32_000));
    p.pass(r.at(T0 + 33_000));
    p.pass(r.at(T0 + 34_000));
    assert_eq!(r.row(&second.id), None, "it went once the turn stayed over");
    assert_eq!(r.returns(), 2);
}

#[test]
fn the_unsure_row_keeps_holding_while_the_session_waits_on_a_dialog() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    r.files.set_status(IDLE, "waiting");
    r.send("job-idle", "add id to the lists");
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 10_000));
    assert_eq!(r.returns(), 1, "nothing is typed into a dialog");
    assert_eq!(r.row(&first).unwrap().aside_ms, None);
}

#[test]
fn a_second_unsure_in_a_row_still_holds_the_rows_behind_it() {
    let r = rig();
    let (_, mut p) = unsure_first(&r, "first message");
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let second = r.send("job-idle", "second message");
    for step in 1..5 {
        p.pass(r.at(T0 + step * 1_000));
    }
    assert_eq!(
        state(&r, &second.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    let third = r.send("job-idle", "third message");
    p.pass(r.at(T0 + 10_000));
    p.pass(r.at(T0 + 20_000));
    assert_eq!(state(&r, &third.id), Some((State::Queued, None)));
    assert!(!r.typed().contains("third message"));
    assert_eq!(r.row(&second.id).unwrap().aside_ms, None);
}

#[test]
fn a_held_row_behind_does_not_make_the_unsure_row_step_aside() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    let second = r.send("job-idle", "add id to the lists");
    call(&r, "outbox.hold", json!({"id": second.id, "on": true})).unwrap();
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 10_000));
    assert_eq!(r.returns(), 1);
    assert_eq!(r.row(&first).unwrap().aside_ms, None);
}

#[test]
fn a_row_backing_off_behind_does_not_make_the_unsure_row_step_aside() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    let second = r.send("job-idle", "add id to the lists");
    r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET next_ms = {} WHERE id = '{}'",
        T0 + 60_000,
        second.id
    ));
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 10_000));
    assert_eq!(r.returns(), 1);
    assert_eq!(r.row(&first).unwrap().aside_ms, None);
}

#[test]
fn the_unsure_row_is_never_typed_again_by_itself() {
    let r = rig();
    let (_, mut p) = unsure_first(&r, "is it right");
    r.send("job-idle", "done");
    for step in 1..6 {
        p.pass(r.at(T0 + step * 60_000));
    }
    assert_eq!(r.typed().matches("is it right").count(), 1);
}

#[test]
fn a_row_queued_behind_an_unsure_one_leaves_only_by_being_typed_or_by_remove() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    let kept = r.send_with("job-idle", "typed one", true);
    let taken_back = r.send_with("job-idle", "taken back", true);
    let last = r.send_with("job-idle", "typed two", true);
    let gone = call(&r, "outbox.remove", json!({"id": taken_back.id})).unwrap();
    assert_eq!(gone, json!({"removed": true}));
    for step in 1..5 {
        p.pass(r.at(T0 + step * 1_000));
    }
    let typed = r.typed();
    for row in [&kept, &last] {
        assert_eq!(r.row(&row.id), None, "{} was sent", row.text);
        assert!(typed.contains(&row.text), "{} was typed: {typed}", row.text);
    }
    assert!(!typed.contains("taken back"), "{typed}");
    assert_eq!(r.row(&taken_back.id), None);
    assert!(r.row(&first).is_some());
}

#[test]
fn rows_that_join_into_one_message_are_all_in_what_was_typed() {
    let r = rig();
    let (_, mut p) = unsure_first(&r, "is it right");
    let rows: Vec<_> = ["add id", "to the accounts list", "done"]
        .iter()
        .map(|text| r.send("job-idle", text))
        .collect();
    for step in 1..5 {
        p.pass(r.at(T0 + step * 1_000));
    }
    let typed = r.typed();
    for (row, text) in rows.iter().zip(["add id", "to the accounts list", "done"]) {
        assert_eq!(r.row(&row.id), None);
        assert!(typed.contains(text), "{text} was typed: {typed}");
    }
    let removed = r.room.named("outbox.removed");
    assert!(
        removed.iter().all(|g| g["why"] != "removed"),
        "nothing was removed as the person's call: {removed:?}"
    );
}

#[test]
fn readings_of_idle_far_apart_do_not_add_up() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "is it right");
    let second = r.send("job-idle", "add id to the lists");
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 2_000));
    p.pass(r.at(T0 + 10_000));
    assert_eq!(state(&r, &second.id), Some((State::Queued, None)));
    assert_eq!(r.row(&first).unwrap().aside_ms, None);
    p.pass(r.at(T0 + 11_000));
    p.pass(r.at(T0 + 12_000));
    assert_eq!(state(&r, &second.id), Some((State::Queued, None)));
    p.pass(r.at(T0 + 13_000));
    assert_eq!(r.row(&second.id), None, "three seconds of steady idle");
}
