use super::*;
use crate::acting_ops_tests::PROMPT;
use crate::outbox_postman::{Next, TICK_MS};
use crate::outbox_testkit::{rig, Rig, IDLE, NO_BOX, T0};
use notchling_engine::outbox::TURN_SETTLE_MS;
use notchling_engine::outbox::{Verdict, UNREADABLE_AFTER_MS, UNSURE_HOLD_MS};
use notchling_engine::reply::keys;
use notchling_engine::reply::keys::ENTER;
use notchling_engine::terminal::input_box::holds_message;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

const BOX_RULE: &str = "────────────────────────────────────────";

fn box_holding(r: &Rig, text: &str) {
    let held = format!("❯ {text}");
    r.show(&[BOX_RULE, &held, BOX_RULE, "  ? for shortcuts"]);
}

fn ignored(r: &Rig, text: &str, tried_at: i64) -> (Postman, Row) {
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let head = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(tried_at));
    let tried = r.row(&head.id).unwrap();
    assert_eq!(tried.tries, 1);
    assert_eq!((tried.state, tried.why), (State::Waiting, Some(Why::Retry)));
    r.deaf.store(false, Ordering::SeqCst);
    (p, tried)
}

fn retry_passes(r: &Rig, p: &mut Postman, from: i64) {
    for step in 0..=4 {
        p.pass(r.at(from + step * TICK_MS));
    }
}

fn a_message_the_session_took_late_is_sent_by_the_retry_not_typed_again(text: &str, tried_at: i64) {
    let r = rig();
    let (mut p, tried) = ignored(&r, text, tried_at);
    r.verifier.late.store(true, Ordering::SeqCst);
    let writes = r.writes.lock().unwrap().len();
    let (enters, attaches) = (r.returns(), r.attaches());
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    assert_eq!(r.row(&tried.id), None, "the transcript shows it: Sent");
    assert_eq!(r.writes.lock().unwrap().len(), writes, "not one key more");
    assert_eq!((r.returns(), r.attaches()), (enters, attaches));
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.last().unwrap()["why"], "sent");
}

#[test]
fn a_message_the_session_took_late_is_sent_by_the_retry_not_typed_again_plain() {
    a_message_the_session_took_late_is_sent_by_the_retry_not_typed_again("is it right", T0);
}

#[test]
fn a_retry_that_finds_its_text_still_in_the_box_clears_it_and_types_again() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    box_holding(&r, "is it right");
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    assert_eq!(r.row(&tried.id), None);
    let again = r.writes.lock().unwrap()[writes..].to_vec();
    assert_eq!(again.first(), Some(&keys::clear_input()));
    assert_eq!(again.iter().filter(|w| *w == ENTER).count(), 1);
}

fn an_emptied_box_is_verified_before_typing(verdict: Verdict, then: Option<(State, Option<Why>)>) {
    an_emptied_box_is_verified_before_typing_for(verdict, then, "is it right", T0);
}

fn an_emptied_box_is_verified_before_typing_for(
    verdict: Verdict,
    then: Option<(State, Option<Why>)>,
    text: &str,
    tried_at: i64,
) {
    let r = rig();
    let (mut p, tried) = ignored(&r, text, tried_at);
    assert!(tried.unsent, "the ignored Enter is kept on the row");
    r.verifier.waits.lock().unwrap().push_back(verdict);
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    assert_eq!(r.row(&tried.id).map(|row| (row.state, row.why)), then);
    assert_eq!(r.writes.lock().unwrap().len(), writes, "nothing typed");
}

#[test]
fn a_box_that_lost_our_text_and_a_transcript_that_shows_it_is_sent_without_typing() {
    an_emptied_box_is_verified_before_typing(Verdict::Sent, None);
}

#[test]
fn a_box_that_lost_our_text_with_nothing_in_the_transcript_is_unsure_not_typed_over() {
    an_emptied_box_is_verified_before_typing(
        Verdict::Unsure,
        Some((State::Unsure, Some(Why::NotFound))),
    );
}

#[test]
fn an_ignored_enter_is_retried_three_times_and_the_fourth_ending_is_unsure() {
    let r = rig();
    let text = "is it right";
    r.deaf.store(true, Ordering::SeqCst);
    for _ in 0..4 {
        r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    }
    let head = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for tries in 1..=NOT_SENT_RETRIES {
        retry_passes(&r, &mut p, at);
        let row = r.row(&head.id).unwrap();
        assert_eq!((row.state, row.why), (State::Waiting, Some(Why::Retry)));
        assert_eq!(row.tries, tries);
        assert_eq!(row.aside_at_ms, None);
        at = row.next_ms.unwrap();
        box_holding(&r, text);
    }
    retry_passes(&r, &mut p, at);
    let row = r.row(&head.id).unwrap();
    assert_eq!((row.state, row.why), (State::Unsure, Some(Why::NotFound)));
    assert_eq!(row.tries, NOT_SENT_RETRIES + 1);
    assert!(row
        .aside_at_ms
        .is_some_and(|aside| aside >= at + UNSURE_HOLD_MS));
    let enters = r.returns();
    retry_passes(&r, &mut p, at + 10_000);
    assert_eq!(r.returns(), enters, "an Unsure row is never typed again");
}

fn nothing_typed_since(r: &Rig, writes: usize) {
    assert_eq!(r.writes.lock().unwrap().len(), writes, "nothing typed");
}

fn held(r: &Rig, id: &str) {
    let row = r.row(id).unwrap();
    assert_eq!(
        (row.state, row.why, row.unsent),
        (State::Waiting, Some(Why::Retry), true)
    );
}

fn unsure(r: &Rig, id: &str) {
    let row = r.row(id).unwrap();
    assert_eq!((row.state, row.why), (State::Unsure, Some(Why::NotFound)));
    assert!(!row.unsent, "an Unsure row drops the mark");
}

fn verdicts(r: &Rig, verdict: Verdict, n: usize) {
    let mut waits = r.verifier.waits.lock().unwrap();
    waits.clear();
    waits.extend(std::iter::repeat_n(verdict, n));
}

const HOURS_MS: i64 = 3 * 60 * 60 * 1_000;

fn ignored_command(r: &Rig, text: &str) -> (Postman, Row) {
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let head = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(T0 + TURN_SETTLE_MS));
    (p, r.row(&head.id).unwrap())
}

#[test]
fn an_ignored_enter_on_a_slash_command_ends_the_row_in_the_box_at_once() {
    for text in [
        "/clear",
        "  /compact",
        "/clear\nthen the tests",
        "!git push",
        "! ls -la",
    ] {
        let r = rig();
        let (_p, row) = ignored_command(&r, text);
        assert_eq!(
            (row.state, row.why, row.tries, row.unsent, row.next_ms),
            (State::Box, Some(Why::NotCleared), 1, false, None),
            "{text}"
        );
        let boxed = r.ops.note(&row.id).unwrap_or_default();
        assert!(holds_message(&boxed, text), "{text}: {boxed:?}");
        assert_eq!(r.returns(), 1, "{text}: one Enter, never a second");
    }
}

#[test]
fn a_slash_command_in_the_box_is_never_typed_again_however_much_later() {
    let r = rig();
    let (mut p, row) = ignored_command(&r, "/clear");
    let writes = r.writes.lock().unwrap().len();
    let (enters, attaches) = (r.returns(), r.attaches());
    r.deaf.store(false, Ordering::SeqCst);
    for waits in 0..2 {
        verdicts(&r, Verdict::Sent, 4);
        retry_passes(&r, &mut p, T0 + HOURS_MS * (waits + 1));
        let later = r.row(&row.id).unwrap();
        assert_eq!(
            (later.state, later.why),
            (State::Box, Some(Why::NotCleared))
        );
        assert_eq!(later.tries, 1);
    }
    assert_eq!(r.writes.lock().unwrap().len(), writes, "nothing typed");
    assert_eq!((r.returns(), r.attaches()), (enters, attaches));
}

#[test]
fn a_slash_command_in_the_box_is_sent_again_only_when_the_person_asks_to_clear_the_box() {
    let r = rig();
    let (mut p, row) = ignored_command(&r, "/clear");
    r.deaf.store(false, Ordering::SeqCst);
    r.ops.resend_row(&row.id).unwrap();
    verdicts(&r, Verdict::Sent, 1);
    retry_passes(&r, &mut p, T0 + HOURS_MS);
    assert_eq!(r.row(&row.id), None, "typed once more and sent");
    assert_eq!(r.returns(), 2);
}

#[test]
fn a_message_behind_a_slash_command_left_in_the_box_waits_and_nothing_is_typed() {
    let r = rig();
    let (mut p, row) = ignored_command(&r, "/clear");
    r.safe.note(IDLE, false);
    let behind = r.send("job-idle", "what changed?");
    let writes = r.writes.lock().unwrap().len();
    r.deaf.store(false, Ordering::SeqCst);
    retry_passes(&r, &mut p, T0 + HOURS_MS);
    assert_eq!(r.row(&row.id).map(|row| row.state), Some(State::Box));
    let waiting = r.row(&behind.id).unwrap();
    assert_eq!((waiting.state, waiting.tries), (State::Queued, 0));
    nothing_typed_since(&r, writes);
}

#[test]
fn a_restarted_daemon_leaves_a_command_in_the_box_alone() {
    for text in ["/clear", "!git push"] {
        let r = rig();
        let (p, row) = ignored_command(&r, text);
        drop(p);
        let mut p = r.restarted().postman(IDLE);
        let writes = r.writes.lock().unwrap().len();
        r.deaf.store(false, Ordering::SeqCst);
        verdicts(&r, Verdict::Sent, 4);
        retry_passes(&r, &mut p, T0 + HOURS_MS);
        let later = r.row(&row.id).unwrap();
        assert_eq!((later.state, later.tries), (State::Box, 1), "{text}");
        nothing_typed_since(&r, writes);
    }
}

#[test]
fn a_slash_command_whose_enter_the_session_took_is_not_put_in_the_box() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Sent);
    let head = r.send("job-idle", "/clear");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(T0 + TURN_SETTLE_MS));
    assert_eq!(r.row(&head.id), None);
}

#[test]
fn a_late_check_still_pending_looks_again_soon_and_never_types() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    verdicts(&r, Verdict::Pending, 1);
    let writes = r.writes.lock().unwrap().len();
    let due = tried.next_ms.unwrap();
    let next = p.pass(r.at(due));
    assert!(
        matches!(next, Next::At(at) if at <= due + TICK_MS),
        "{next:?}"
    );
    held(&r, &tried.id);
    nothing_typed_since(&r, writes);
    verdicts(&r, Verdict::Unsure, 1);
    retry_passes(&r, &mut p, due + TICK_MS);
    unsure(&r, &tried.id);
    nothing_typed_since(&r, writes);
}

#[test]
fn a_box_that_cannot_be_read_holds_the_retry_and_the_mark_until_it_can() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    r.show(NO_BOX);
    verdicts(&r, Verdict::Sent, 8);
    let writes = r.writes.lock().unwrap().len();
    let due = tried.next_ms.unwrap();
    retry_passes(&r, &mut p, due);
    held(&r, &tried.id);
    nothing_typed_since(&r, writes);
    r.show(PROMPT);
    verdicts(&r, Verdict::Unsure, 1);
    retry_passes(&r, &mut p, due + 5 * TICK_MS);
    unsure(&r, &tried.id);
    nothing_typed_since(&r, writes);
}

#[test]
fn a_box_unreadable_past_its_grace_is_verified_never_typed() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    r.show(NO_BOX);
    verdicts(&r, Verdict::Unsure, 1);
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap() + UNREADABLE_AFTER_MS);
    unsure(&r, &tried.id);
    nothing_typed_since(&r, writes);
}

#[test]
fn a_verdict_the_store_refused_is_looked_at_again_by_the_next_daemon_not_typed() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    r.ops.store().run_sql_for_tests(
        "CREATE TRIGGER stuck BEFORE UPDATE OF state ON outbox WHEN NEW.state = 'unsure'
         BEGIN SELECT RAISE(ABORT, 'stuck'); END;",
    );
    verdicts(&r, Verdict::Unsure, 1);
    let writes = r.writes.lock().unwrap().len();
    let due = tried.next_ms.unwrap();
    retry_passes(&r, &mut p, due);
    let _ = p.end();
    drop(p);
    held(&r, &tried.id);
    r.ops.store().run_sql_for_tests("DROP TRIGGER stuck;");
    verdicts(&r, Verdict::Unsure, 1);
    let restarted = r.restarted();
    let mut p = restarted.postman(IDLE);
    retry_passes(&r, &mut p, due + 10 * TICK_MS);
    unsure(&r, &tried.id);
    nothing_typed_since(&r, writes);
}

#[test]
fn a_restarted_daemon_still_checks_the_box_before_it_types_a_retry() {
    let r = rig();
    let (p, tried) = ignored(&r, "is it right", T0);
    drop(p);
    let restarted = r.restarted();
    let mut p = restarted.postman(IDLE);
    verdicts(&r, Verdict::Unsure, 1);
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    unsure(&r, &tried.id);
    nothing_typed_since(&r, writes);
}

#[test]
fn a_retry_that_types_again_keeps_the_mark_while_it_sends_and_drops_it_when_sent() {
    let r = rig();
    let (mut p, tried) = ignored(&r, "is it right", T0);
    box_holding(&r, "is it right");
    let sending = r.ops.clone();
    let id = tried.id.clone();
    let seen = Arc::new(Mutex::new(None));
    let during = seen.clone();
    *r.verifier.during_wait.lock().unwrap() = Some(Box::new(move || {
        *during.lock().unwrap() = sending
            .store()
            .get(&id)
            .unwrap()
            .map(|row| (row.state, row.unsent));
    }));
    verdicts(&r, Verdict::Sent, 1);
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    assert_eq!(*seen.lock().unwrap(), Some((State::Sending, true)));
    assert_eq!(r.row(&tried.id), None);
    assert_eq!(
        r.returns(),
        2,
        "typed once more, after the box showed the text"
    );
}

#[test]
fn an_ignored_enter_leaves_a_command_in_the_box_only_when_the_box_still_held_it() {
    let r = rig();
    let end = TryEnd::Failed(Failure::NotSent);
    for text in ["/clear", "!git push"] {
        let row = r.send("job-idle", text);
        assert!(
            super::ignored_command(&row, &end, &Some("held".into())),
            "{text}"
        );
        assert!(
            !super::ignored_command(&row, &end, &None),
            "{text}: box seen empty"
        );
        let other = TryEnd::Failed(Failure::Mixed);
        assert!(
            !super::ignored_command(&row, &other, &Some("held".into())),
            "{text}"
        );
    }
    let plain = r.send("job-idle", "is it right");
    assert!(!super::ignored_command(&plain, &end, &Some("held".into())));
}
