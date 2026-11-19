use super::*;
use crate::acting_ops_tests::{app, PROMPT};
use crate::op_group::OpGroup;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, DIALOG, IDLE, T0};
use notchling_engine::outbox::{Verdict, MAX_TEXT};
use notchling_engine::transcript::queue_state::QueueState;
use serde_json::{json, Value};

fn call(r: &Rig, op: &str, args: Value) -> Value {
    r.ops.call(&app(), op, &args).unwrap()
}

fn gone(r: &Rig) -> Vec<(String, String, Option<String>)> {
    r.room
        .named("outbox.removed")
        .iter()
        .map(|e| {
            (
                e["id"].as_str().unwrap().to_string(),
                e["why"].as_str().unwrap().to_string(),
                e["into"].as_str().map(str::to_string),
            )
        })
        .collect()
}

fn state(r: &Rig, id: &str) -> Option<(State, Option<Why>)> {
    r.row(id).map(|row| (row.state, row.why))
}

#[test]
fn rows_waiting_behind_the_head_go_with_it_as_one_message() {
    let r = rig();
    let one = r.send("job-idle", "one");
    let two = r.send("job-idle", "two");
    let three = r.send("job-idle", "three");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.returns(), 1, "one message, one Return");
    assert!(r.typed().contains("one\n\ntwo\n\nthree"), "{}", r.typed());
    assert_eq!(r.row(&one.id), None);
    assert_eq!(r.row(&two.id), None);
    assert_eq!(r.row(&three.id), None);
    let left = gone(&r);
    assert!(
        left.contains(&(two.id.clone(), "joined".into(), Some(one.id.clone()))),
        "{left:?}"
    );
    assert!(
        left.contains(&(three.id.clone(), "joined".into(), Some(one.id.clone()))),
        "{left:?}"
    );
    assert!(
        left.contains(&(one.id.clone(), "sent".into(), None)),
        "{left:?}"
    );
}

#[test]
fn rows_behind_our_earlier_prompt_in_claude_codes_queue_go_together_once_it_is_taken() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: vec!["one".into()],
        unknown: 0,
    });
    let two = r.send("job-idle", "two");
    let three = r.send("job-idle", "three");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(state(&r, &two.id), Some((State::Waiting, Some(Why::Queue))));
    assert_eq!(state(&r, &three.id).map(|s| s.0), Some(State::Queued));
    assert_eq!(r.returns(), 0);
    *r.files.queue.lock().unwrap() = Some(QueueState::default());
    p.pass(r.at(T0 + 1_000));
    assert_eq!(r.returns(), 1, "typed once, not twice a step apart");
    assert!(r.typed().contains("two\n\nthree"), "{}", r.typed());
    assert_eq!(
        gone(&r)
            .iter()
            .filter(|(id, why, into)| id == &three.id
                && why == "joined"
                && into.as_deref() == Some(two.id.as_str()))
            .count(),
        1
    );
}

#[test]
fn a_slash_command_row_stops_the_join() {
    let r = rig();
    let one = r.send("job-idle", "one");
    let command = r.send("job-idle", "/compact");
    let three = r.send("job-idle", "three");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(r.typed().contains("one"));
    assert!(!r.typed().contains("one\n\n"), "{}", r.typed());
    assert_eq!(r.row(&one.id), None);
    let kept = r.row(&command.id).expect("the command is its own row");
    assert_eq!(kept.text, "/compact");
    assert_eq!(r.row(&three.id).unwrap().text, "three");
}

#[test]
fn a_shell_line_row_is_not_joined_into_a_message() {
    let r = rig();
    r.send("job-idle", "one");
    let shell = r.send("job-idle", "!ls");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\n!ls"), "{}", r.typed());
    assert_eq!(r.row(&shell.id).map(|row| row.text), Some("!ls".into()));
}

#[test]
fn a_row_with_other_options_stops_the_join() {
    let r = rig();
    r.send("job-idle", "one");
    let other = r
        .ops
        .send(&crate::outbox_ops::SendArgs {
            id: "job-idle".into(),
            text: "two".into(),
            clear_box: false,
            options: Some(json!({"model": "opus"})),
            via: notchling_engine::outbox::Via::Chat,
        })
        .unwrap();
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\ntwo"), "{}", r.typed());
    assert_eq!(r.row(&other.id).unwrap().text, "two");
}

#[test]
fn a_clear_box_row_stops_the_join() {
    let r = rig();
    r.send("job-idle", "one");
    let clearing = r.send_with("job-idle", "two", true);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\ntwo"), "{}", r.typed());
    assert_eq!(r.row(&clearing.id).unwrap().text, "two");
}

#[test]
fn a_row_put_back_by_retry_stops_the_join() {
    let r = rig();
    r.send("job-idle", "one");
    let back = r.send("job-idle", "two");
    r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET tries = 1 WHERE id = '{}'",
        back.id
    ));
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\ntwo"), "{}", r.typed());
}

#[test]
fn a_row_being_edited_is_not_joined() {
    let r = rig();
    r.send("job-idle", "one");
    let editing = r.send("job-idle", "two");
    call(&r, "outbox.hold", json!({"id": editing.id, "on": true}));
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\ntwo"), "{}", r.typed());
    assert_eq!(r.row(&editing.id).unwrap().text, "two");
}

#[test]
fn the_join_stops_before_the_size_limit() {
    let r = rig();
    let big = "x".repeat(MAX_TEXT - 10);
    r.send("job-idle", &big);
    let next = r.send("job-idle", "this will not fit");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        r.row(&next.id).map(|row| row.text),
        Some("this will not fit".into())
    );
}

#[test]
fn a_row_saved_while_the_head_waits_for_a_dialog_stays_its_own_row_until_the_take() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    let head = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        state(&r, &head.id),
        Some((State::Waiting, Some(Why::Dialog)))
    );
    let saved = call(
        &r,
        "session.send",
        json!({"id": "job-idle", "text": "and this"}),
    );
    assert_ne!(saved["outboxId"], json!(head.id), "its own row");
    assert!(saved.get("combined").is_none(), "{saved}");
    assert_eq!(r.row(&head.id).unwrap().text, "hi");
    r.show(PROMPT);
    p.pass(r.at(T0 + 31_000));
    assert_eq!(r.returns(), 1);
    assert!(r.typed().contains("hi\n\nand this"), "{}", r.typed());
}

#[test]
fn two_session_sends_in_a_row_are_two_rows_a_person_can_edit_and_remove() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let first = call(
        &r,
        "session.send",
        json!({"id": "job-idle", "text": "first"}),
    );
    let second = call(
        &r,
        "session.send",
        json!({"id": "job-idle", "text": "second"}),
    );
    assert_ne!(first["outboxId"], second["outboxId"]);
    assert_eq!(second["position"], 2);
    let id = second["outboxId"].as_str().unwrap();
    call(
        &r,
        "outbox.edit",
        json!({"id": id, "text": "second, better"}),
    );
    assert_eq!(r.row(id).unwrap().text, "second, better");
    call(&r, "outbox.remove", json!({"id": id}));
    assert_eq!(r.row(id), None);
}

#[test]
fn the_head_keeps_its_id_and_tells_the_app_how_many_messages_it_carries() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let one = r.send("job-idle", "one");
    r.send("job-idle", "two");
    r.send("job-idle", "three");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let carried = r
        .row(&one.id)
        .expect("in Claude Code's queue, still its row");
    assert_eq!(carried.state, State::InQueue);
    assert_eq!(carried.text, "one\n\ntwo\n\nthree");
    let wire = crate::outbox_rows::wire(&carried, 1, None);
    assert_eq!(wire["joined"], json!(3), "{wire}");
    let plain = crate::outbox_rows::wire(&r.send("job-idle", "later"), 2, None);
    assert!(plain.get("joined").is_none());
}

#[test]
fn rows_of_another_session_are_not_joined() {
    let r = rig();
    r.send("job-idle", "one");
    let other = r.send("job-idle2", "elsewhere");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(!r.typed().contains("one\n\nelsewhere"), "{}", r.typed());
    assert!(r.row(&other.id).is_some());
}
