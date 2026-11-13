use crate::acting_ops_tests::{app, sessions, setup, Setup, PROMPT};
use notchling_engine::reply::held::START_GRACE_MS;
use notchling_local_api::{ops, topics};
use serde_json::{json, Value};

const SCREEN: &[&str] = PROMPT;

fn reply(s: &Setup, id: &str, text: &str) -> Value {
    s.ops
        .call(&app(), ops::SESSION_REPLY, &json!({"id": id, "text": text}))
        .unwrap()
}

fn finish(s: &Setup, id: &str) {
    let mut list = s.ops.sessions.lock().unwrap();
    for row in list.as_array_mut().unwrap() {
        if row["id"] == json!(id) {
            row["group"] = json!("completed");
        }
    }
}

fn typed(s: &Setup) -> String {
    s.writes.lock().unwrap().concat()
}

#[test]
fn a_reply_to_a_working_row_is_held_and_nothing_is_typed() {
    let s = setup(true, SCREEN, sessions());
    let out = reply(&s, "job-a", "two");
    assert_eq!(
        out,
        json!({"sent": true, "state": "queued", "waiting": 1, "held": "two"})
    );
    assert!(s.writes.lock().unwrap().is_empty());
    assert!(s.attached.lock().unwrap().is_empty());
    assert_eq!(
        s.room.named(topics::SESSION_HELD),
        vec![json!({"id": "job-a", "text": "two"})]
    );
}

#[test]
fn a_second_reply_joins_the_held_one() {
    let s = setup(true, SCREEN, sessions());
    reply(&s, "job-a", "two");
    let out = reply(&s, "job-a", "three");
    assert_eq!(out["held"], json!("two\n\nthree"));
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two\n\nthree"));
    let events = s.room.named(topics::SESSION_HELD);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], json!({"id": "job-a", "text": "two\n\nthree"}));
}

#[test]
fn the_held_text_is_typed_once_when_the_turn_ends() {
    let s = setup(true, SCREEN, sessions());
    reply(&s, "job-a", "two");
    reply(&s, "job-a", "three");
    s.ops.flush_held(1_000);
    assert!(s.writes.lock().unwrap().is_empty(), "still working");
    finish(&s, "job-a");
    s.ops.flush_held(2_000);
    let written = typed(&s);
    assert!(written.contains("two\n\nthree"), "{written}");
    assert_eq!(s.ops.held_text("job-a"), None);
    let events = s.room.named(topics::SESSION_HELD);
    assert_eq!(
        events.last().unwrap(),
        &json!({"id": "job-a", "text": null})
    );
    let before = s.writes.lock().unwrap().len();
    s.ops.flush_held(3_000 + START_GRACE_MS);
    assert_eq!(s.writes.lock().unwrap().len(), before, "sent once");
}

#[test]
fn a_reply_to_a_finished_row_is_typed_at_once() {
    let s = setup(true, SCREEN, sessions());
    let out = reply(&s, "job-idle", "go");
    assert_eq!(out, json!({"sent": true}));
    assert!(typed(&s).contains("go"));
    assert!(s.room.named(topics::SESSION_HELD).is_empty());
}

#[test]
fn nothing_is_flushed_while_acting_is_off() {
    let s = setup(true, SCREEN, sessions());
    reply(&s, "job-a", "two");
    finish(&s, "job-a");
    s.ops.acting.set(false);
    s.ops.flush_held(100_000);
    assert!(s.writes.lock().unwrap().is_empty());
    assert_eq!(s.ops.held_text("job-a").as_deref(), Some("two"));
    s.ops.acting.set(true);
    s.ops.flush_held(100_000);
    assert!(typed(&s).contains("two"));
}
