use super::*;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, BUSY, IDLE, T0};
use notchling_engine::outbox::{gate, Gate, State, Why, TURN_SETTLE_MS};
use notchling_engine::reply::keys::timing::{ATTACH_QUIET_MS, KEY_PAUSE_MS};
use std::sync::atomic::Ordering;

fn taken(r: &Rig, id: &str, text: &str) -> Row {
    let row = r.send(id, text);
    r.ops.store().take(&row.id, T0).unwrap().unwrap()
}

#[test]
fn a_missing_job_counts_from_when_it_was_first_missed() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.files.vanish(IDLE);
    let first = r.courier.facts(&row, T0);
    assert_eq!(first.job, Job::Missing { since_ms: T0 });
    let later = r.courier.facts(&row, T0 + 59_999);
    assert_eq!(later.job, Job::Missing { since_ms: T0 });
    assert!(matches!(
        gate(&row, &later),
        Gate::Wait {
            why: Why::Retry,
            ..
        }
    ));
    let gone = r.courier.facts(&row, T0 + 60_000);
    assert_eq!(gate(&row, &gone), Gate::NeedsUser(State::Gone));
}

#[test]
fn a_session_that_takes_no_replies_any_more_is_gone_at_once() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    let mut agent = r.files.rows.lock().unwrap()[IDLE].clone();
    agent.command = Some("make".into());
    r.files.put(IDLE, agent);
    let facts = r.courier.facts(&row, T0);
    assert_eq!(gate(&row, &facts), Gate::NeedsUser(State::Gone));
}

#[test]
fn the_live_status_and_the_job_state_come_from_the_files() {
    let r = rig();
    let row = r.send("job-a", "hi");
    assert_eq!(
        r.courier.facts(&row, T0).job,
        Job::Live {
            status: Some("busy".into())
        }
    );
    r.files.set_state(BUSY, "stopped");
    assert_eq!(r.courier.facts(&row, T0).job, Job::Stopped);
}

#[test]
fn an_unknown_queue_line_makes_the_session_safe() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: Vec::new(),
        unknown: 1,
    });
    let row = r.send("job-idle", "hi");
    let facts = r.courier.facts(&row, T0);
    assert!(facts.safe);
    assert_eq!(facts.turn_over_since_ms, Some(T0));
    assert!(matches!(
        gate(&row, &facts),
        Gate::Wait { why: Why::Turn, .. }
    ));
    let settled = r.courier.facts(&row, T0 + TURN_SETTLE_MS);
    assert_eq!(gate(&row, &settled), Gate::Go { guarded: true });
}

#[test]
fn a_command_waits_for_the_turn_end_to_stand() {
    let r = rig();
    let row = r.send("job-a", "/compact");
    let mut agent = r.files.rows.lock().unwrap()[BUSY].clone();
    agent.at_prompt = true;
    r.files.put(BUSY, agent);
    r.files.restarted.store(true, Ordering::SeqCst);
    let running = r.courier.facts(&row, T0);
    assert_eq!(running.turn_over_since_ms, None);
    r.files.restarted.store(false, Ordering::SeqCst);
    assert_eq!(
        r.courier.facts(&row, T0 + 1_000).turn_over_since_ms,
        Some(T0 + 1_000)
    );
    let early = r.courier.facts(&row, T0 + 1_000 + TURN_SETTLE_MS - 1);
    assert!(matches!(
        gate(&row, &early),
        Gate::Wait { why: Why::Turn, .. }
    ));
    let late = r.courier.facts(&row, T0 + 1_000 + TURN_SETTLE_MS);
    assert_eq!(gate(&row, &late), Gate::Go { guarded: true });
}

#[test]
fn a_second_row_reuses_the_open_attach() {
    let r = rig();
    let one = taken(&r, "job-idle", "one");
    assert_eq!(r.courier.deliver(&one, false).0, TryEnd::Ok);
    r.ops.store().delete(&one.id).unwrap();
    let two = taken(&r, "job-idle", "two");
    let (end, tried, _) = r.courier.deliver(&two, false);
    assert_eq!(end, TryEnd::Ok);
    assert!(tried.pasted && tried.entered);
    assert_eq!(r.attaches(), 1);
    assert_eq!(r.returns(), 2);
    r.courier.close(IDLE);
    assert_eq!(r.courier.open(), 0);
}

#[test]
fn a_person_typing_beside_ours_is_mixed_with_no_return() {
    let r = rig();
    r.human
        .lock()
        .unwrap()
        .push((ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "x".to_string()));
    let row = taken(&r, "job-idle", "two");
    let (end, tried, note) = r.courier.deliver(&row, false);
    assert_eq!(end, TryEnd::Failed(Failure::Mixed));
    assert!(tried.pasted && !tried.entered);
    assert!(note.is_some_and(|n| n.contains('x')));
    assert_eq!(r.returns(), 0);
    assert_eq!(r.courier.open(), 0, "closed after a failure");
}

#[test]
fn a_turn_that_starts_after_the_reading_stops_the_first_key() {
    let r = rig();
    let mut agent = r.files.rows.lock().unwrap()[BUSY].clone();
    agent.at_prompt = true;
    r.files.put(BUSY, agent);
    let row = taken(&r, "job-a", "/compact");
    r.files.restarted.store(true, Ordering::SeqCst);
    let (end, tried, _) = r.courier.deliver(&row, true);
    assert_eq!(end, TryEnd::NotIdle);
    assert!(!tried.pasted && !tried.entered);
    assert_eq!(r.attaches(), 1, "it did attach");
    assert!(r.typed().is_empty(), "not one key");
}

#[test]
fn a_session_with_no_job_row_is_not_attached() {
    let r = rig();
    let row = taken(&r, "job-idle", "hi");
    r.files.vanish(IDLE);
    assert_eq!(r.courier.deliver(&row, false).0, TryEnd::AttachFailed);
    assert_eq!(r.attaches(), 0);
}

#[test]
fn a_starting_screen_that_never_settles_is_a_retry_with_its_reason() {
    let r = rig_with(Options {
        screen: crate::outbox_testkit::NO_BOX,
        ..Options::default()
    });
    let row = taken(&r, "job-idle", "hi");
    let (end, tried, note) = r.courier.deliver(&row, false);
    assert_eq!(end, TryEnd::Failed(Failure::NoInput));
    assert!(!tried.pasted);
    assert_eq!(note, Some(Failure::NoInput.to_string()));
}

#[test]
fn refuse_waits_and_never_types() {
    let r = rig();
    let row = r.send("codex:t1", "hi");
    let facts = Refuse.facts(&row, T0);
    assert!(matches!(
        gate(&row, &facts),
        Gate::Wait {
            why: Why::Retry,
            ..
        }
    ));
    assert!(!Refuse.verifies());
    assert_eq!(Refuse.deliver(&row, false).0, TryEnd::AttachFailed);
}
