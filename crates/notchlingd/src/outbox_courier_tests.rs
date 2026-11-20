use super::*;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, BUSY, IDLE, T0};
use notchling_engine::outbox::{
    gate, Gate, State, Why, DONE_STOPS_AFTER_TRIES, QUEUE_STALE_MS, SLOW_READ_MS, TURN_SETTLE_MS,
    UNREADABLE_AFTER_MS,
};
use notchling_engine::reply::keys::timing::{ATTACH_QUIET_MS, KEY_PAUSE_MS};
use notchling_engine::sessions::fresh::Ended;
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

fn checking(since_ms: i64) -> Job {
    Job::Unknown { since_ms }
}

#[test]
fn a_stopped_state_with_a_live_process_is_not_a_stopped_session() {
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.set_state(BUSY, "stopped");
    r.files
        .processes
        .lock()
        .unwrap()
        .insert(BUSY.into(), (4242, Some(1)));
    assert_eq!(r.courier.facts(&row, T0).job, checking(T0));
    let later = r.courier.facts(&row, T0 + 60_000);
    assert_ne!(later.job, Job::Stopped);
    assert!(matches!(
        gate(&row, &r.courier.facts(&row, T0)),
        Gate::Wait {
            why: Why::Checking,
            ..
        }
    ));
}

#[test]
fn a_real_exit_is_stopped_at_once() {
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.set_state(BUSY, "stopped");
    let facts = r.courier.facts(&row, T0);
    assert_eq!(facts.job, Job::Stopped);
    assert_eq!(gate(&row, &facts), Gate::NeedsUser(State::Stopped));
}

#[test]
fn a_read_that_timed_out_is_unknown_then_unreadable_and_never_stopped() {
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.read_as(BUSY, Ended::Unknown);
    let first = r.courier.facts(&row, T0);
    assert_eq!(first.job, checking(T0));
    assert!(matches!(
        gate(&row, &first),
        Gate::Wait {
            why: Why::Checking,
            ..
        }
    ));
    let long = r.courier.facts(&row, T0 + UNREADABLE_AFTER_MS);
    assert_eq!(long.job, checking(T0));
    assert!(matches!(
        gate(&row, &long),
        Gate::Wait {
            why: Why::Unreadable,
            ..
        }
    ));
    let hour = r.courier.facts(&row, T0 + 3_600_000);
    assert!(matches!(
        gate(&row, &hour),
        Gate::Wait {
            why: Why::Unreadable,
            ..
        }
    ));
}

#[test]
fn a_late_read_never_counts_as_proof_of_a_stop() {
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.set_state(BUSY, "stopped");
    r.files.reads_take(&r.clock, SLOW_READ_MS);
    let late = r.courier.facts(&row, T0);
    assert_eq!(late.job, checking(T0));
    assert!(matches!(
        gate(&row, &late),
        Gate::Wait {
            why: Why::Checking,
            ..
        }
    ));
    r.files.reads_take(&r.clock, SLOW_READ_MS - 1);
    assert_eq!(r.courier.facts(&row, T0 + 1_000).job, Job::Stopped);
}

#[test]
fn a_state_file_caught_in_a_rename_is_unknown_and_the_next_read_decides() {
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.set_state(BUSY, "stopped");
    r.files.read_as(BUSY, Ended::Unknown);
    assert_eq!(r.courier.facts(&row, T0).job, checking(T0));
    r.files.ended.lock().unwrap().clear();
    r.files.set_state(BUSY, "working");
    let back = r.courier.facts(&row, T0 + 1_000);
    assert_eq!(
        back.job,
        Job::Live {
            status: Some("busy".into())
        }
    );
    assert!(matches!(gate(&row, &back), Gate::Go { .. }));
}

#[test]
fn a_done_job_with_no_process_is_tried_then_stopped_when_the_tries_fail() {
    let r = rig();
    let mut row = r.send("job-a", "hi");
    r.files.set_state(BUSY, "done");
    let facts = r.courier.facts(&row, T0);
    assert_eq!(facts.job, Job::Done);
    assert_eq!(gate(&row, &facts), Gate::Go { guarded: false });
    row.tries = DONE_STOPS_AFTER_TRIES;
    assert_eq!(gate(&row, &facts), Gate::NeedsUser(State::Stopped));
    r.files
        .processes
        .lock()
        .unwrap()
        .insert(BUSY.into(), (4242, Some(1)));
    let alive = r.courier.facts(&row, T0 + 1);
    assert!(matches!(alive.job, Job::Live { .. }));
    assert!(matches!(gate(&row, &alive), Gate::Go { .. }));
}

#[test]
fn an_unreadable_job_waits_in_the_outbox_with_its_reason_and_goes_on_when_it_reads() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.files.read_as(IDLE, Ended::Unknown);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        r.row(&row.id).map(|row| (row.state, row.why)),
        Some((State::Waiting, Some(Why::Checking)))
    );
    p.pass(r.at(T0 + UNREADABLE_AFTER_MS));
    assert_eq!(
        r.row(&row.id).map(|row| (row.state, row.why)),
        Some((State::Waiting, Some(Why::Unreadable)))
    );
    p.pass(r.at(T0 + 3_600_000));
    assert_eq!(r.row(&row.id).unwrap().state, State::Waiting);
    r.files.ended.lock().unwrap().clear();
    p.pass(r.at(T0 + 3_601_000));
    assert_eq!(r.row(&row.id), None, "read again, typed");
    assert!(r.typed().contains("hi"));
}

#[test]
fn a_session_that_really_ended_shows_stopped_in_the_outbox() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.files.set_state(IDLE, "stopped");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.row(&row.id).unwrap().state, State::Stopped);
    assert!(r.typed().is_empty());
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
fn a_listed_queue_entry_counts_the_calm_from_the_first_look() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: vec!["lost".into()],
        unknown: 0,
    });
    let row = r.send("job-idle", "hi");
    let first = r.courier.facts(&row, T0);
    assert_eq!(first.queue_waiting, 1);
    assert!(!first.safe);
    assert_eq!(first.turn_over_since_ms, Some(T0));
    assert!(matches!(
        gate(&row, &first),
        Gate::Wait {
            why: Why::Queue,
            ..
        }
    ));
    let mut at = T0;
    while at < T0 + QUEUE_STALE_MS - 1_000 {
        at += 1_000;
        r.courier.facts(&row, at);
    }
    let early = r.courier.facts(&row, T0 + QUEUE_STALE_MS - 1);
    assert!(matches!(
        gate(&row, &early),
        Gate::Wait {
            why: Why::Queue,
            ..
        }
    ));
    let stale = r.courier.facts(&row, T0 + QUEUE_STALE_MS);
    assert_eq!(gate(&row, &stale), Gate::Go { guarded: true });
}

#[test]
fn a_turn_in_between_restarts_the_queue_count() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: vec!["lost".into()],
        unknown: 0,
    });
    let row = r.send("job-a", "hi");
    let mut agent = r.files.rows.lock().unwrap()[BUSY].clone();
    agent.at_prompt = true;
    r.files.put(BUSY, agent);
    r.files.set_status(BUSY, "idle");
    assert_eq!(r.courier.facts(&row, T0).turn_over_since_ms, Some(T0));
    r.files.restarted.store(true, Ordering::SeqCst);
    assert_eq!(r.courier.facts(&row, T0 + 6_000).turn_over_since_ms, None);
    r.files.restarted.store(false, Ordering::SeqCst);
    let again = r.courier.facts(&row, T0 + QUEUE_STALE_MS);
    assert_eq!(again.turn_over_since_ms, Some(T0 + QUEUE_STALE_MS));
    assert!(matches!(
        gate(&row, &again),
        Gate::Wait {
            why: Why::Queue,
            ..
        }
    ));
}

#[test]
fn a_gap_between_readings_restarts_the_calm_count() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: vec!["lost".into()],
        unknown: 0,
    });
    let row = r.send("job-idle", "hi");
    assert_eq!(r.courier.facts(&row, T0).turn_over_since_ms, Some(T0));
    let hour = T0 + 3_600_000;
    let back = r.courier.facts(&row, hour);
    assert_eq!(back.turn_over_since_ms, Some(hour));
    assert!(matches!(
        gate(&row, &back),
        Gate::Wait {
            why: Why::Queue,
            ..
        }
    ));
    let mut at = hour;
    while at < hour + QUEUE_STALE_MS - 1_000 {
        at += 1_000;
        assert_eq!(r.courier.facts(&row, at).turn_over_since_ms, Some(hour));
    }
    let stale = r.courier.facts(&row, hour + QUEUE_STALE_MS);
    assert_eq!(gate(&row, &stale), Gate::Go { guarded: true });
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

#[test]
fn a_dialog_before_any_paste_notes_nothing() {
    let r = rig_with(Options {
        screen: crate::outbox_testkit::DIALOG,
        ..Options::default()
    });
    let row = taken(&r, "job-idle", "hi");
    let (end, tried, _) = r.courier.deliver(&row, false);
    assert_eq!(end, TryEnd::Failed(Failure::Dialog));
    assert!(!tried.pasted);
    assert!(r.acting.attempts.recent("job-idle", T0 + 1_000).is_empty());
}

#[test]
fn a_pasted_try_notes_the_text_it_typed() {
    let r = rig();
    r.human
        .lock()
        .unwrap()
        .push((ATTACH_QUIET_MS + KEY_PAUSE_MS + 10, "x".to_string()));
    let row = taken(&r, "job-idle", "two");
    let (end, _, _) = r.courier.deliver(&row, false);
    assert_eq!(end, TryEnd::Failed(Failure::Mixed));
    assert_eq!(r.acting.attempts.recent("job-idle", T0 + 1_000), ["two"]);
}

#[test]
fn an_early_failure_closes_the_open_attach() {
    let r = rig();
    let one = taken(&r, "job-idle", "one");
    assert_eq!(r.courier.deliver(&one, false).0, TryEnd::Ok);
    assert_eq!(r.courier.open(), 1);
    r.ops.store().delete(&one.id).unwrap();
    let two = taken(&r, "job-idle", "two");
    r.files.vanish(IDLE);
    assert_eq!(r.courier.deliver(&two, false).0, TryEnd::AttachFailed);
    assert_eq!(r.courier.open(), 0);
}
