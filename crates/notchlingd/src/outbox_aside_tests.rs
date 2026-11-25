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
fn an_unsure_head_holds_the_row_behind_until_the_hold_runs_out() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    r.files.set_status(IDLE, "busy");
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS - 1_000));
    assert_eq!(state(&r, &second.id), Some((State::Queued, None)));
    assert_eq!(r.returns(), 1, "nothing is typed behind it yet");
    assert_eq!(state(&r, &first).map(|s| s.0), Some(State::Unsure));
}

#[test]
fn an_unsure_head_steps_aside_after_the_hold_and_the_row_behind_goes() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    assert_eq!(
        r.row(&second.id),
        None,
        "the row behind was typed and settled"
    );
    assert_eq!(r.returns(), 2);
    assert!(r.typed().contains("second message"));
    assert_eq!(
        state(&r, &first),
        Some((State::Unsure, Some(Why::NotFound))),
        "the unsure row stays, with its buttons"
    );
    let kept = r.ops.store().get(&first).unwrap().unwrap();
    assert_eq!(kept.text, "first message");
}

#[test]
fn the_row_that_stepped_aside_is_never_typed_again_by_itself() {
    let r = rig();
    let (_, mut p) = unsure_first(&r, "first message");
    r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    p.pass(r.at(T0 + 10 * UNSURE_HOLD_MS));
    let typed = r.typed();
    assert_eq!(typed.matches("first message").count(), 1, "{typed}");
    assert_eq!(r.returns(), 2);
}

#[test]
fn a_second_unsure_in_a_row_holds_and_the_rows_behind_it_wait() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    assert_eq!(
        state(&r, &second.id),
        Some((State::Unsure, Some(Why::NotFound))),
        "the second message went unsure as well"
    );
    let third = r.send("job-idle", "third message");
    p.pass(r.at(T0 + 3 * UNSURE_HOLD_MS));
    assert_eq!(
        state(&r, &third.id),
        Some((State::Queued, None)),
        "two in a row mean something systematic: nothing goes behind it"
    );
    assert!(!r.typed().contains("third message"));
    assert_eq!(state(&r, &first).map(|s| s.0), Some(State::Unsure));
}

#[test]
fn a_message_sent_in_between_ends_the_streak() {
    let r = rig();
    let (_, mut p) = unsure_first(&r, "first message");
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    assert_eq!(r.row(&second.id), None, "settled as sent");
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let third = r.send("job-idle", "third message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 2_000));
    assert_eq!(state(&r, &third.id).map(|s| s.0), Some(State::Unsure));
    let fourth = r.send("job-idle", "fourth message");
    p.pass(r.at(T0 + 2 * UNSURE_HOLD_MS + 3_000));
    assert_eq!(
        r.row(&fourth.id),
        None,
        "the third was the first unsure of a new streak"
    );
}

#[test]
fn mark_as_sent_removes_an_unsure_row_as_sent_and_logs_the_persons_call() {
    let r = rig();
    let (first, _p) = unsure_first(&r, "first message");
    let done = call(&r, "outbox.remove", json!({"id": first, "sent": true})).unwrap();
    assert_eq!(done["removed"], json!(true));
    assert_eq!(r.row(&first), None);
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
    let told: Vec<String> = crate::event_assert::of(&r.sink, "session.outbox")
        .iter()
        .filter_map(|e| crate::event_assert::text(e, "then"))
        .collect();
    assert_eq!(
        told.last().map(String::as_str),
        Some("marked_sent"),
        "{told:?}"
    );
}

#[test]
fn mark_as_sent_refuses_a_row_that_is_not_waiting_for_the_person() {
    let r = rig();
    let queued = r.send("job-idle", "still queued");
    let refused = call(&r, "outbox.remove", json!({"id": queued.id, "sent": true}));
    assert!(
        refused.is_err(),
        "a queued row is removed, not marked as sent"
    );
    assert!(r.row(&queued.id).is_some());
}

#[test]
fn retry_on_a_row_that_stepped_aside_puts_it_back_in_the_queue_once() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    call(&r, "outbox.resend", json!({"id": first})).unwrap();
    assert_eq!(state(&r, &first), Some((State::Queued, None)));
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 2_000));
    assert_eq!(r.row(&first), None);
    assert_eq!(r.typed().matches("first message").count(), 2);
}

#[test]
fn an_unsure_row_tells_the_app_when_it_steps_aside() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    let holding = crate::outbox_rows::wire(&r.row(&first).unwrap(), 1, None, None);
    assert_eq!(
        holding["asideAtMs"],
        json!(T0 + UNSURE_HOLD_MS),
        "{holding}"
    );
    assert!(holding.get("asideMs").is_none());
    r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    let aside = crate::outbox_rows::wire(&r.row(&first).unwrap(), 1, None, None);
    assert!(aside.get("asideMs").is_some(), "{aside}");
    assert!(aside.get("asideAtMs").is_none());
}

#[test]
fn edit_holds_the_head_so_the_postman_does_not_type_it() {
    let r = rig();
    let row = r.send("job-idle", "draft one");
    let held = call(&r, "outbox.hold", json!({"id": row.id, "on": true}));
    assert!(held.is_ok(), "{held:?}");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.returns(), 0, "a held row is not typed");
    assert!(r.row(&row.id).is_some());
    let behind = r.send("job-idle", "draft two");
    p.pass(r.at(T0 + 5_000));
    assert_eq!(state(&r, &behind.id).map(|s| s.0), Some(State::Queued));
    assert_eq!(r.returns(), 0, "the row behind waits too");
}

#[test]
fn saving_an_edit_puts_the_new_text_in_the_same_place_and_releases_the_hold() {
    let r = rig();
    let row = r.send("job-idle", "draft one");
    let behind = r.send("job-idle", "draft two");
    call(&r, "outbox.hold", json!({"id": row.id, "on": true})).unwrap();
    call(
        &r,
        "outbox.edit",
        json!({"id": row.id, "text": "draft one, better"}),
    )
    .unwrap();
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(r.typed().contains("draft one, better"));
    assert_eq!(r.row(&row.id), None);
    assert_eq!(r.row(&behind.id), None);
}

#[test]
fn a_hold_ends_by_itself_after_its_lease() {
    let r = rig();
    let row = r.send("job-idle", "draft one");
    call(&r, "outbox.hold", json!({"id": row.id, "on": true})).unwrap();
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0 + 9 * 60_000));
    assert_eq!(r.returns(), 0);
    p.pass(r.at(T0 + 11 * 60_000));
    assert_eq!(r.row(&row.id), None, "the lease ran out and the row went");
    assert_eq!(r.returns(), 1);
}

#[test]
fn releasing_the_hold_lets_the_row_go() {
    let r = rig();
    let row = r.send("job-idle", "draft one");
    call(&r, "outbox.hold", json!({"id": row.id, "on": true})).unwrap();
    call(&r, "outbox.hold", json!({"id": row.id, "on": false})).unwrap();
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.row(&row.id), None);
}
