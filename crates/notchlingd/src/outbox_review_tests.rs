use super::*;
use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use crate::outbox_testkit::{rig, Rig, IDLE, T0};
use notchling_engine::outbox::{QueuedVerdict, Verdict, UNSURE_HOLD_MS};
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

fn reply_in_thread(r: &Rig, text: &'static str) -> Receiver<Result<Value, ApiError>> {
    let (tx, rx) = channel();
    let acting = r.acting.clone();
    std::thread::spawn(move || {
        let out = acting.call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": text}),
        );
        let _ = tx.send(out);
    });
    rx
}

fn rows_in(r: &Rig) -> usize {
    r.ops.store().list(Some(IDLE)).unwrap().len()
}

fn until(what: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn answer(rx: &Receiver<Result<Value, ApiError>>) -> Result<Value, ApiError> {
    rx.recv_timeout(Duration::from_secs(5)).expect("an answer")
}

fn call(r: &Rig, op: &str, args: Value) -> Result<Value, ApiError> {
    r.ops.call(&app(), op, &args)
}

fn unsure_first(r: &Rig, text: &str) -> (String, Postman) {
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    (row.id, p)
}

#[test]
fn a_reply_waiting_behind_a_head_is_not_joined_and_gets_its_own_answer() {
    let r = rig();
    r.send("job-idle", "one");
    let waiting = reply_in_thread(&r, "two");
    assert!(until(|| rows_in(&r) == 2));
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(answer(&waiting).unwrap(), json!({"sent": true}));
    assert_eq!(r.returns(), 2, "two messages, two Returns");
    assert!(!r.typed().contains("one\n\ntwo"), "{}", r.typed());
    let removed = r.room.named("outbox.removed");
    assert!(
        removed.iter().all(|e| e["why"] != "joined"),
        "nothing was joined: {removed:?}"
    );
}

#[test]
fn a_reply_that_ends_unsure_takes_back_only_its_own_words() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let waiting = reply_in_thread(&r, "reply words");
    assert!(until(|| rows_in(&r) == 1));
    let later = r.send("job-idle", "later words");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let err = answer(&waiting).unwrap_err();
    assert!(
        err.message.contains("may already be in the session"),
        "{err:?}"
    );
    if let Some(kept) = r.row(&later.id) {
        assert_eq!(kept.text, "later words");
        assert_eq!(kept.joined, 0);
    }
    assert!(
        !r.typed().contains("reply words\n\nlater words"),
        "{}",
        r.typed()
    );
    let removed = r.room.named("outbox.removed");
    assert!(removed.iter().all(|e| e["why"] != "joined"), "{removed:?}");
}

#[test]
fn a_chat_row_does_not_join_the_head_when_the_head_has_a_reply_waiting() {
    let r = rig();
    let waiting = reply_in_thread(&r, "reply words");
    assert!(until(|| rows_in(&r) == 1));
    let behind = r.send("job-idle", "behind words");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(answer(&waiting).unwrap(), json!({"sent": true}));
    assert_eq!(r.returns(), 2);
    assert!(r.typed().contains("behind words"));
    assert_eq!(r.row(&behind.id), None, "it went on its own");
}

#[test]
fn retry_of_an_aside_row_waits_while_a_later_row_holds_as_unsure() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    assert_eq!(r.returns(), 2);
    assert_eq!(
        r.row(&second.id).map(|row| row.state),
        Some(State::Unsure),
        "the second went unsure as well and holds"
    );
    call(&r, "outbox.resend", json!({"id": first})).unwrap();
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 2_000));
    assert_eq!(
        r.returns(),
        2,
        "the retry did not cut into the second's hold"
    );
    assert_eq!(r.row(&first).map(|row| row.state), Some(State::Queued));
    call(&r, "outbox.remove", json!({"id": second.id, "sent": true})).unwrap();
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 3_000));
    assert_eq!(r.returns(), 3, "now the retried row goes");
    assert_eq!(r.row(&first), None);
}

#[test]
fn retry_of_an_aside_row_waits_while_a_later_row_is_in_claude_codes_queue() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let second = r.send("job-idle", "second message");
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 1_000));
    assert_eq!(r.row(&second.id).map(|row| row.state), Some(State::InQueue));
    call(&r, "outbox.resend", json!({"id": first})).unwrap();
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 2_000));
    assert_eq!(
        r.returns(),
        2,
        "the retry waits for the later message to be taken"
    );
    r.verifier
        .watches
        .lock()
        .unwrap()
        .push_back(QueuedVerdict::Sent);
    p.pass(r.at(T0 + UNSURE_HOLD_MS + 3_000));
    assert_eq!(r.row(&second.id), None);
    assert_eq!(r.returns(), 3);
    assert_eq!(r.row(&first), None);
}

#[test]
fn the_look_record_of_a_row_that_is_gone_is_pruned() {
    let r = rig();
    let (first, mut p) = unsure_first(&r, "first message");
    p.looked.insert("a-row-long-gone".to_string(), (1, 2));
    p.looked.insert(first.clone(), (1, 2));
    p.pass(r.at(T0 + 1_000));
    assert!(!p.looked.contains_key("a-row-long-gone"), "{:?}", p.looked);
    assert!(p.looked.contains_key(&first), "a row still there is kept");
}
