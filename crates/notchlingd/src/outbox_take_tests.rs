use super::*;
use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use serde_json::{json, Value};

fn racing(r: &Rig, courier: &Arc<Stops>, op: &'static str, args: Value) {
    let ops = r.ops.clone();
    *courier.before_take.lock().unwrap() = Some(Box::new(move || {
        ops.call(&app(), op, &args).unwrap();
    }));
}

#[test]
fn a_resend_between_the_gate_and_the_take_is_not_undone_by_a_stop() {
    let not_sent = (TryEnd::Failed(Failure::NotSent), TYPED);
    let courier = stops(idle_job(), not_sent.clone(), 2, true);
    let r = rig_stopped_by(&courier);
    *r.verifier.seen.lock().unwrap() = Some(Seen {
        taken: 4,
        enqueued: 2,
    });
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let first = "is the check green now?";
    let row = r.send("job-idle", first);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let tried = r.row(&row.id).unwrap();
    assert_eq!(
        (tried.why, tried.unsent, tried.seen_before),
        (Some(Why::Retry), true, Some(4)),
        "{tried:?}"
    );
    let behind = r.send("job-idle", "and the lint?");
    let at = due(&r, &row.id);
    racing(&r, &courier, "outbox.resend", json!({"id": row.id}));
    p.pass(r.at(at));
    assert_eq!(courier.delivered(), 2, "the stop cut the resend's wake");
    let left = r.row(&row.id).unwrap();
    assert_eq!(
        (left.state, left.why, left.tries, left.unsent),
        (State::Queued, None, 1, false),
        "the row is as the resend left it: {left:?}"
    );
    assert_eq!(
        (left.seen_before, left.queued_before, left.attempt_ms),
        (None, None, None),
        "try 1's baseline does not come back: {left:?}"
    );

    let joined = "is the check green now?\n\nand the lint?";
    assert_eq!((left.text.as_str(), r.row(&behind.id)), (joined, None));
    let after = stops(idle_job(), not_sent, usize::MAX, true);
    let mut p = r.restarted_with(after.clone()).postman(IDLE);
    p.pass(r.at(at));
    assert_eq!(after.typed(), vec![joined], "sent again, once");
}

fn waiting_untried(r: &Rig, text: &str) -> Row {
    let row = r.send("job-idle", text);
    let (from, why) = ([State::Queued], Some(Why::Dialog));
    let waits = r
        .ops
        .store()
        .set_state(&row.id, &from, State::Waiting, why, None, T0);
    assert_eq!(waits, Ok(true));
    r.row(&row.id).unwrap()
}

#[test]
fn a_same_text_edit_of_an_untried_row_is_not_undone_by_a_stop() {
    let timeout = (TryEnd::Failed(Failure::Timeout), NOTHING);
    let courier = stops(idle_job(), timeout, 1, false);
    let r = rig_stopped_by(&courier);
    let text = "is the check green now?";
    let row = waiting_untried(&r, text);
    assert_eq!((row.why, row.tries), (Some(Why::Dialog), 0));
    racing(
        &r,
        &courier,
        "outbox.edit",
        json!({"id": row.id, "text": text}),
    );
    r.postmen.postman(IDLE).pass(r.at(T0 + 1_000));
    assert_eq!(courier.delivered(), 1, "the stop cut the wake");
    let left = r.row(&row.id).unwrap();
    assert_eq!(
        (left.state, left.why, left.tries),
        (State::Queued, None, 0),
        "the row is as the edit left it: {left:?}"
    );
}

#[test]
fn a_row_that_moved_between_the_gate_and_the_take_is_gated_again_and_sent_once() {
    let typed = (TryEnd::Ok, TYPED);
    let courier = stops(idle_job(), typed, usize::MAX, false);
    let r = rig_stopped_by(&courier);
    let text = "is the check green now?";
    let row = waiting_untried(&r, text);
    racing(
        &r,
        &courier,
        "outbox.edit",
        json!({"id": row.id, "text": text}),
    );
    r.postmen.postman(IDLE).pass(r.at(T0 + 1_000));
    assert_eq!(courier.typed(), vec![text], "typed once, in the same pass");
    assert!(r.row(&row.id).is_none(), "the reply counts as sent");
    let gone = r.room.named("outbox.removed");
    assert_eq!((gone.len(), &gone[0]["why"]), (1, &json!("sent")));
}

#[test]
fn a_stop_after_a_join_leaves_no_earlier_baseline_on_the_joined_text() {
    let pasted = Tried {
        pasted: true,
        entered: false,
    };
    let timeout = (TryEnd::Failed(Failure::Timeout), pasted);
    let courier = stops(idle_job(), timeout, 2, true);
    let r = rig_stopped_by(&courier);
    *r.verifier.seen.lock().unwrap() = Some(Seen {
        taken: 4,
        enqueued: 2,
    });
    *r.verifier.anchor.lock().unwrap() = Some(Anchor {
        file: Some("/tmp/none/idle.jsonl".to_string()),
        slack_ms: 1_500,
    });
    let head = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let tried = r.row(&head.id).unwrap();
    assert_eq!(
        (tried.why, tried.unsent, tried.seen_before),
        (Some(Why::Retry), false, Some(4)),
        "{tried:?}"
    );
    assert!(tried.anchor.is_some() && tried.attempt_ms.is_some());
    let behind = r.send("job-idle", "and the lint?");
    let at = due(&r, &head.id);
    p.pass(r.at(at));
    assert_eq!(courier.delivered(), 2, "the stop cut try 2's wake");
    let joined = "is the check green now?\n\nand the lint?";
    let left = r.row(&head.id).unwrap();
    assert_eq!((left.text.as_str(), r.row(&behind.id)), (joined, None));
    assert_eq!(
        (left.state, left.why, left.tries, left.next_ms),
        (State::Waiting, Some(Why::Retry), 1, None),
        "{left:?}"
    );
    assert_eq!(
        (left.seen_before, left.queued_before, left.attempt_ms),
        (None, None, None),
        "try 1's baseline was not taken for this text: {left:?}"
    );
    assert_eq!(left.anchor, None);

    r.verifier.late.store(true, Ordering::SeqCst);
    let after = stops(idle_job(), (TryEnd::Ok, TYPED), usize::MAX, true);
    let mut p = r.restarted_with(after.clone()).postman(IDLE);
    p.pass(r.at(at));
    assert_eq!(
        after.typed(),
        vec![joined],
        "the joined text is typed, not counted as landed"
    );
    assert!(r.row(&head.id).is_none(), "the reply counts as sent");
}
