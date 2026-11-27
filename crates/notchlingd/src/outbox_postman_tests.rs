use super::*;
use crate::outbox_courier::Courier;
use crate::outbox_testkit::{rig, rig_with, Options, Rig, BUSY, DIALOG, DRAFT, IDLE, NO_BOX, T0};
use notchling_engine::outbox::{Facts, Kind, Tried, TryEnd, UNSURE_HOLD_MS};
use notchling_engine::reply::exec::Failure;
use notchling_engine::reply::keys;
use notchling_engine::transcript::queue_state::QueueState;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

fn state(r: &Rig, id: &str) -> Option<(State, Option<Why>)> {
    r.row(id).map(|row| (row.state, row.why))
}

#[test]
fn idle_session_message_is_typed_and_removed() {
    let r = rig();
    let row = r.send("job-idle", "hello there");
    let mut p = r.postmen.postman(IDLE);
    assert_eq!(p.pass(r.at(T0)), Next::At(T0 + ATTACH_IDLE_MS));
    assert_eq!(r.row(&row.id), None);
    assert!(r.typed().contains("hello there"));
    assert_eq!(r.returns(), 1);
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["why"], "sent");
}

#[test]
fn busy_session_message_is_typed_at_once() {
    let r = rig();
    let row = r.send("job-a", "and also this");
    let mut p = r.postmen.postman(BUSY);
    p.pass(r.at(T0));
    assert_eq!(r.row(&row.id), None);
    assert!(r.typed().contains("and also this"));
}

#[test]
fn three_rows_typed_in_order_through_one_attach() {
    let r = rig();
    for (text, clear) in [("one", false), ("two", true), ("three", true)] {
        r.send_with("job-idle", text, clear);
    }
    let mut p = r.postmen.postman(IDLE);
    assert_eq!(p.pass(r.at(T0)), Next::At(T0 + ATTACH_IDLE_MS));
    assert_eq!(r.attaches(), 1);
    assert_eq!(r.returns(), 3);
    let typed = r.typed();
    let at = |t: &str| typed.find(t).unwrap();
    assert!(at("one") < at("two") && at("two") < at("three"));
    assert_eq!(r.courier.open(), 1);
    assert_eq!(
        p.pass(r.at(T0 + ATTACH_IDLE_MS - 1)),
        Next::At(T0 + ATTACH_IDLE_MS)
    );
    assert_eq!(r.courier.open(), 1);
    assert_eq!(p.pass(r.at(T0 + ATTACH_IDLE_MS)), Next::Done);
    assert_eq!(r.courier.open(), 0);
    r.send("job-idle", "four");
    p.pass(r.at(T0 + ATTACH_IDLE_MS + 5));
    assert_eq!(r.attaches(), 2);
}

#[test]
fn dialog_on_screen_waits_and_goes_after() {
    let r = rig_with(Options {
        screen: DIALOG,
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        state(&r, &row.id),
        Some((State::Waiting, Some(Why::Dialog)))
    );
    assert_eq!(r.returns(), 0);
    let attaches = r.attaches();
    p.pass(r.at(T0 + 5_000));
    assert_eq!(r.attaches(), attaches, "the dialog is waited out");
    r.show(crate::acting_ops_tests::PROMPT);
    p.pass(r.at(T0 + 31_000));
    assert_eq!(r.row(&row.id), None);
    assert_eq!(r.returns(), 1);
}

#[test]
fn waiting_status_waits_for_the_dialog() {
    let r = rig();
    r.files.set_status(IDLE, "waiting");
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        state(&r, &row.id),
        Some((State::Waiting, Some(Why::Dialog)))
    );
    assert_eq!(r.attaches(), 0);
    r.files.set_status(IDLE, "idle");
    p.pass(r.at(T0 + 1_000));
    assert_eq!(r.row(&row.id), None);
}

#[test]
fn foreign_box_text_stops_at_box_with_text() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    let row = r.send("job-idle", "hello");
    let mut p = r.postmen.postman(IDLE);
    assert_eq!(p.pass(r.at(T0)), Next::Done);
    assert_eq!(
        state(&r, &row.id),
        Some((State::Box, Some(Why::NotCleared)))
    );
    assert!(r.typed().is_empty(), "not even the clear keys");
    let changed = r.room.named("outbox.changed");
    let last = changed.last().unwrap();
    assert_eq!(last["row"]["state"], "box");
    assert_eq!(last["row"]["box"], "my own draft");
}

#[test]
fn box_text_goes_to_the_row_note() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    let row = r.send("job-idle", "hello");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert_eq!(r.ops.note(&row.id).as_deref(), Some("my own draft"));
}

#[test]
fn clear_box_and_send_types_once() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    let row = r.send("job-idle", "hello");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    r.ops
        .store()
        .requeue(&row.id, &[State::Box], true, T0 + 1)
        .unwrap();
    p.pass(r.at(T0 + 2_000));
    assert_eq!(r.row(&row.id), None);
    assert_eq!(r.returns(), 1);
    assert_eq!(r.typed().matches("hello").count(), 1);
}

#[test]
fn rows_behind_a_box_head_wait() {
    let r = rig_with(Options {
        screen: DRAFT,
        ..Options::default()
    });
    let first = r.send("job-idle", "first");
    let second = r.send_with("job-idle", "second", true);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(T0 + 1_000));
    assert_eq!(state(&r, &first.id).unwrap().0, State::Box);
    let behind = r.row(&second.id).unwrap();
    assert_eq!((behind.state, behind.tries), (State::Queued, 0));
    assert_eq!(r.attaches(), 1);
}

#[test]
fn rows_behind_an_in_queue_head_wait() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let first = r.send("job-idle", "first");
    let second = r.send_with("job-idle", "second", true);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(state(&r, &first.id), Some((State::InQueue, None)));
    p.pass(r.at(T0 + 1_000));
    p.pass(r.at(T0 + 2_000));
    assert!(!r.typed().contains("second"));
    assert_eq!(r.row(&second.id).unwrap().state, State::Queued);
    assert_eq!(r.verifier.watched_at.lock().unwrap().len(), 2);
}

#[test]
fn in_queue_head_is_judged_at_next_look_ms() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let row = r.send("job-idle", "first");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let look = T0 + 500;
    *r.verifier.next_look.lock().unwrap() = Some(look);
    assert_eq!(p.pass(r.at(T0 + 100)), Next::At(look));
    r.verifier
        .watches
        .lock()
        .unwrap()
        .push_back(QueuedVerdict::Unsure);
    p.pass(r.at(look));
    assert_eq!(*r.verifier.watched_at.lock().unwrap(), [T0 + 100, look]);
    assert_eq!(
        state(&r, &row.id),
        Some((State::Unsure, Some(Why::Dropped)))
    );
}

#[test]
fn no_input_retries_with_backoff_never_unsure() {
    let r = rig_with(Options {
        screen: NO_BOX,
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for (tries, wait) in [(1, 30_000), (2, 60_000), (3, 120_000), (4, 300_000)] {
        p.pass(r.at(at));
        let now = r.row(&row.id).unwrap();
        assert_eq!((now.state, now.why), (State::Waiting, Some(Why::Retry)));
        assert_eq!(now.tries, tries);
        assert_eq!(now.next_ms, Some(at + wait));
        p.pass(r.at(at + wait - 1));
        assert_eq!(r.row(&row.id).unwrap().tries, tries, "not before next_ms");
        at += wait;
    }
    assert_eq!(r.returns(), 0);
    assert!(r.ops.note(&row.id).is_some(), "the failure is shown");
}

struct TimesOut {
    inner: Arc<crate::outbox_courier::ClaudeCourier>,
    first: std::sync::atomic::AtomicBool,
}

impl Courier for TimesOut {
    fn facts(&self, row: &Row, now_ms: i64) -> Facts {
        self.inner.facts(row, now_ms)
    }

    fn deliver(&self, row: &Row, guarded: bool) -> (TryEnd, Tried, Option<String>) {
        let (end, tried, note) = self.inner.deliver(row, guarded);
        match (end, self.first.swap(false, Ordering::SeqCst)) {
            (TryEnd::Failed(Failure::NotSent), true) => {
                (TryEnd::Failed(Failure::Timeout), tried, note)
            }
            (end, _) => (end, tried, note),
        }
    }

    fn close(&self, session: &str) {
        self.inner.close(session)
    }

    fn verifies(&self) -> bool {
        true
    }
}

#[test]
fn send_again_clears_own_text_in_box_and_types_once() {
    let r = rig();
    let ticks = r.clock.clone();
    let now: crate::outbox_courier::Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let courier = Arc::new(TimesOut {
        inner: r.courier.clone(),
        first: std::sync::atomic::AtomicBool::new(true),
    });
    let postmen = Postmen::build(
        r.ops.clone(),
        Couriers::claude_only(courier),
        r.verifier.clone(),
        r.acting_switch(),
        now,
    );
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let row = r.send("job-idle", "ours");
    let mut p = postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(
        state(&r, &row.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    p.pass(r.at(T0 + ATTACH_IDLE_MS + 1));
    r.deaf.store(false, Ordering::SeqCst);
    box_holding(&r, "ours");
    r.ops
        .store()
        .requeue(&row.id, &[State::Unsure], false, T0 + 20_000)
        .unwrap();
    let before = r.writes.lock().unwrap().len();
    p.pass(r.at(T0 + 20_001));
    assert_eq!(r.row(&row.id), None);
    let again: Vec<String> = r.writes.lock().unwrap()[before..].to_vec();
    assert_eq!(again.first(), Some(&keys::clear_input()));
    assert_eq!(again.iter().filter(|w| w.contains("ours")).count(), 1);
}

#[test]
fn the_retry_clears_its_own_text_in_box_and_types_once() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let row = r.send("job-idle", "ours");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(state(&r, &row.id), Some((State::Waiting, Some(Why::Retry))));
    p.pass(r.at(T0 + ATTACH_IDLE_MS + 1));
    r.deaf.store(false, Ordering::SeqCst);
    r.show(&[
        "────────────────────────────────────────",
        "❯ ours",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    let before = r.writes.lock().unwrap().len();
    p.pass(r.at(T0 + 30_000));
    assert_eq!(r.row(&row.id), None);
    let again: Vec<String> = r.writes.lock().unwrap()[before..].to_vec();
    assert_eq!(again.iter().filter(|w| w.contains("ours")).count(), 1);
}

#[test]
fn failure_screen_file_has_no_message_text() {
    let dir = std::env::temp_dir().join(format!("nl-outbox-screen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("reply-screen.txt");
    let r = rig_with(Options {
        screen: DRAFT,
        reply_screen: Some(file.clone()),
        ..Options::default()
    });
    let token = "sk-planted-1234";
    r.send("job-idle", &format!("use {token}"));
    r.postmen.postman(IDLE).pass(r.at(T0));
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(kept.contains("message: "), "{kept}");
    assert!(!kept.contains(token));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stopped_job_is_never_attached() {
    let r = rig();
    r.files.set_state(IDLE, "stopped");
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    assert_eq!(p.pass(r.at(T0)), Next::Done);
    assert_eq!(state(&r, &row.id), Some((State::Stopped, None)));
    assert_eq!(r.attaches(), 0);
}

#[test]
fn resume_and_send_respawns_then_types() {
    let r = rig();
    r.files.set_state(IDLE, "stopped");
    let courier = r.courier.clone();
    r.ops.set_resume(Arc::new(move |row| courier.resume(row)));
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let args = serde_json::json!({"id": row.id, "resume": true});
    r.files.set_state(IDLE, "done");
    crate::op_group::OpGroup::call(
        r.ops.as_ref(),
        &crate::acting_ops_tests::app(),
        notchling_local_api::ops::OUTBOX_RESEND,
        &args,
    )
    .unwrap();
    assert_eq!(
        *r.launched.runs.lock().unwrap(),
        [vec!["claude".to_string(), "respawn".into(), IDLE.into()]]
    );
    p.pass(r.at(T0 + 1_000));
    assert_eq!(r.row(&row.id), None);
    assert!(r.typed().contains("hi"));
}

#[test]
fn missing_job_is_gone_after_60s_and_back_when_it_returns() {
    let r = rig();
    let agent = r.files.vanish(IDLE).unwrap();
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(state(&r, &row.id), Some((State::Waiting, Some(Why::Retry))));
    p.pass(r.at(T0 + 59_999));
    assert_eq!(r.row(&row.id).unwrap().state, State::Waiting);
    assert_eq!(p.pass(r.at(T0 + 60_000)), Next::At(T0 + 70_000));
    assert_eq!(state(&r, &row.id), Some((State::Gone, None)));
    p.pass(r.at(T0 + 70_000));
    assert_eq!(r.row(&row.id).unwrap().state, State::Gone);
    r.files.put(IDLE, agent);
    p.pass(r.at(T0 + 75_000));
    assert_eq!(
        r.row(&row.id).unwrap().state,
        State::Gone,
        "not before 10 s"
    );
    p.pass(r.at(T0 + 80_000));
    assert_eq!(r.row(&row.id), None, "back, queued and typed");
    assert!(r.typed().contains("hi"));
}

struct Plain {
    asked: AtomicUsize,
    end: TryEnd,
    verifies: bool,
}

impl Courier for Plain {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Live { status: None },
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: None,
        }
    }
    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let tried = Tried {
            pasted: false,
            entered: false,
        };
        (self.end.clone(), tried, None)
    }
    fn close(&self, _session: &str) {}
    fn verifies(&self) -> bool {
        self.verifies
    }
}

#[test]
fn courier_that_does_not_verify_is_sent_at_once() {
    let plain = Arc::new(Plain {
        asked: AtomicUsize::new(0),
        end: TryEnd::Ok,
        verifies: false,
    });
    let r = rig_with(Options {
        courier: Some(plain.clone()),
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert_eq!(r.row(&row.id), None);
    assert_eq!(plain.asked.load(Ordering::SeqCst), 1);
    assert_eq!(r.verifier.asked.load(Ordering::SeqCst), 0);
}

#[test]
fn a_cut_try_is_unsure_restart_and_never_tried_again() {
    for verifies in [false, true] {
        let plain = Arc::new(Plain {
            asked: AtomicUsize::new(0),
            end: TryEnd::Cut,
            verifies,
        });
        let r = rig_with(Options {
            courier: Some(plain.clone()),
            ..Options::default()
        });
        let row = r.send("job-idle", "hi");
        let mut p = r.postmen.postman(IDLE);
        p.pass(r.at(T0));
        p.pass(r.at(T0 + 400_000));
        assert_eq!(
            state(&r, &row.id),
            Some((State::Unsure, Some(Why::Restart)))
        );
        assert_eq!(plain.asked.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn seen_none_at_take_stores_zero_baselines() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let row = r.send("job-idle", "hi");
    r.postmen.postman(IDLE).pass(r.at(T0));
    let kept = r.row(&row.id).unwrap();
    assert_eq!((kept.seen_before, kept.queued_before), (Some(0), Some(0)));
}

#[test]
fn baselines_are_stored_before_any_key() {
    let r = rig();
    *r.verifier.seen.lock().unwrap() = Some(Seen {
        enqueued: 2,
        taken: 3,
    });
    r.verifier.waits.lock().unwrap().push_back(Verdict::InQueue);
    let row = r.send("job-idle", "hi");
    r.postmen.postman(IDLE).pass(r.at(T0));
    let kept = r.row(&row.id).unwrap();
    assert_eq!((kept.seen_before, kept.queued_before), (Some(3), Some(2)));
}

#[test]
fn a_waiting_entry_in_claude_codes_queue_holds_ours() {
    let r = rig();
    *r.files.queue.lock().unwrap() = Some(QueueState {
        waiting: vec!["theirs".into()],
        unknown: 0,
    });
    let row = r.send("job-idle", "ours");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(state(&r, &row.id), Some((State::Waiting, Some(Why::Queue))));
    *r.files.queue.lock().unwrap() = Some(QueueState::default());
    p.pass(r.at(T0 + 1_000));
    assert_eq!(r.row(&row.id), None);
}

#[test]
fn acting_off_parks_and_closes_attach() {
    let r = rig();
    r.send("job-idle", "one");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.courier.open(), 1);
    r.acting_switch().set(false);
    let row = r.send("job-idle", "two");
    assert_eq!(p.pass(r.at(T0 + 100)), Next::Done);
    assert_eq!(r.courier.open(), 0);
    assert_eq!(r.row(&row.id).unwrap().state, State::Queued);
    assert!(!r.typed().contains("two"));
}

#[test]
fn acting_off_during_a_verify_types_nothing_more() {
    let r = rig();
    let switch = r.acting_switch();
    *r.verifier.during_wait.lock().unwrap() = Some(Box::new(move || {
        switch.set(false);
    }));
    let first = r.send("job-idle", "first");
    let second = r.send_with("job-idle", "second", true);
    let mut p = r.postmen.postman(IDLE);
    assert_eq!(p.pass(r.at(T0)), Next::Done);
    assert_eq!(r.row(&first.id), None);
    assert!(!r.typed().contains("second"));
    let behind = r.row(&second.id).unwrap();
    assert_eq!((behind.state, behind.tries), (State::Queued, 0));
    assert_eq!(r.courier.open(), 0);
}

fn threaded(r: &Rig) {
    r.ops.set_wake(r.postmen.waker());
    r.postmen.begin();
}

fn until(what: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

#[test]
fn slow_session_does_not_hold_another() {
    let r = rig();
    threaded(&r);
    let typing = r.acting.typing_locks().for_job(IDLE);
    let held = typing.lock().unwrap();
    let slow = r.send("job-idle", "to the slow one");
    let quick = r.send("job-idle2", "to the other");
    assert!(until(|| r.row(&quick.id).is_none()), "the other one went");
    assert_eq!(r.row(&slow.id).unwrap().state, State::Sending);
    drop(held);
    assert!(until(|| r.row(&slow.id).is_none()));
    r.postmen.stop();
    assert_eq!(r.postmen.running(), 0);
}

#[test]
fn a_new_row_wakes_a_thread_and_stop_joins_it() {
    let r = rig();
    threaded(&r);
    let row = r.send("job-idle", "hi");
    assert!(until(|| r.row(&row.id).is_none()));
    assert_eq!(r.postmen.running(), 1, "the attach is still open");
    r.postmen.stop();
    assert_eq!(r.postmen.running(), 0);
    assert_eq!(r.courier.open(), 0, "stop closes the attach");
    let later = r.send("job-idle", "after stop");
    assert_eq!(r.postmen.running(), 0, "a wake after stop starts nothing");
    assert_eq!(r.row(&later.id).unwrap().state, State::Queued);
}

#[test]
fn a_wake_queued_before_stop_types_nothing() {
    let r = rig();
    threaded(&r);
    let typing = r.acting.typing_locks().for_job(IDLE);
    let held = typing.lock().unwrap();
    let first = r.send("job-idle", "first");
    assert!(until(|| r.row(&first.id).unwrap().state == State::Sending));
    let second = r.send("job-idle", "second");
    let postmen = r.postmen.clone();
    let stopping = std::thread::spawn(move || postmen.stop());
    assert!(until(|| lock(&r.postmen.inner.registry).stopped));
    drop(held);
    stopping.join().unwrap();
    assert_eq!(r.postmen.running(), 0);
    assert!(!r.typed().contains("second"));
    assert_eq!(r.row(&second.id).unwrap().state, State::Queued);
}

#[test]
fn a_failed_try_does_not_hold_the_attach() {
    let r = rig_with(Options {
        screen: NO_BOX,
        ..Options::default()
    });
    let row = r.send("job-idle", "hi");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert_eq!(r.row(&row.id).unwrap().next_ms, Some(T0 + 30_000));
    assert_eq!(p.pass(r.at(T0 + 1_000)), Next::At(T0 + 30_000));
}

#[test]
fn acting_off_wakes_a_running_postman_at_once() {
    let r = rig();
    threaded(&r);
    let row = r.send("job-idle", "hi");
    assert!(until(|| r.row(&row.id).is_none()));
    assert!(until(|| r.courier.open() == 1));
    let started = Instant::now();
    r.acting_switch().set(false);
    assert!(until(|| r.postmen.running() == 0));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(r.courier.open(), 0);
    r.postmen.stop();
}

#[test]
fn acting_turned_on_wakes_every_session() {
    let r = rig();
    r.acting_switch().set(false);
    threaded(&r);
    let row = r.send("job-idle", "hi");
    assert!(until(|| r.postmen.running() == 0));
    assert_eq!(r.row(&row.id).unwrap().state, State::Queued);
    r.acting_switch().set(true);
    assert!(until(|| r.row(&row.id).is_none()));
    r.postmen.stop();
}

#[test]
fn postmen_are_capped_and_the_rest_start_as_threads_end() {
    let r = rig();
    threaded(&r);
    let mut rows = Vec::new();
    for n in 0..POSTMEN_MAX {
        rows.push(r.send(&format!("codex:t{n}"), "hi"));
    }
    assert!(until(|| r.postmen.running() == POSTMEN_MAX));
    let waiting = r.send("job-idle", "waits its turn");
    let pending = || lock(&r.postmen.inner.registry).pending.clone();
    assert_eq!(pending(), [IDLE.to_string()], "the rest wait their turn");
    r.ops.store().delete(&rows[0].id).unwrap();
    r.postmen.wake(&rows[0].session);
    assert!(
        until(|| r.row(&waiting.id).is_none()),
        "the pending one ran"
    );
    assert!(pending().is_empty());
    r.at(T0 + ATTACH_IDLE_MS);
    r.postmen.wake(IDLE);
    assert!(until(|| r.postmen.running() == POSTMEN_MAX - 1));
    assert_eq!(r.courier.open(), 0, "and finished");
    r.postmen.stop();
    assert_eq!(r.postmen.running(), 0);
}

#[test]
fn a_verdict_cut_short_by_stop_leaves_the_row_sending() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Pending);
    let row = r.send("job-idle", "hi");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert_eq!(state(&r, &row.id), Some((State::Sending, None)));
}

struct Editing {
    asked: AtomicUsize,
    before: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Courier for Editing {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        let hook = self.before.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Facts {
            now_ms,
            job: Job::Live {
                status: Some("busy".into()),
            },
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: None,
        }
    }
    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let tried = Tried {
            pasted: true,
            entered: true,
        };
        (TryEnd::Ok, tried, None)
    }
    fn close(&self, _session: &str) {}
    fn verifies(&self) -> bool {
        false
    }
}

#[test]
fn an_edit_between_gate_and_take_is_gated_again() {
    let courier = Arc::new(Editing {
        asked: AtomicUsize::new(0),
        before: Mutex::new(None),
    });
    let r = rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    });
    let row = r.send("job-a", "hi");
    let (ops, id) = (r.ops.clone(), row.id.clone());
    *courier.before.lock().unwrap() = Some(Box::new(move || {
        ops.store().edit(&id, "/compact", T0).unwrap();
    }));
    let mut p = r.postmen.postman(BUSY);
    p.pass(r.at(T0));
    assert_eq!(courier.asked.load(Ordering::SeqCst), 0, "nothing typed");
    let now = r.row(&row.id).unwrap();
    assert_eq!((now.text.as_str(), now.kind), ("/compact", Kind::Command));
    assert_eq!((now.state, now.why), (State::Waiting, Some(Why::Turn)));
    assert_eq!(r.returns(), 0);
}

#[test]
fn an_edit_between_gate_and_take_keeps_its_tries() {
    let courier = Arc::new(Editing {
        asked: AtomicUsize::new(0),
        before: Mutex::new(None),
    });
    let r = rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    });
    let row = r.send("job-a", "hi");
    let (ops, id) = (r.ops.clone(), row.id.clone());
    *courier.before.lock().unwrap() = Some(Box::new(move || {
        ops.store().edit(&id, "/compact", T0).unwrap();
    }));
    let mut p = r.postmen.postman(BUSY);
    p.pass(r.at(T0));
    let now = r.row(&row.id).unwrap();
    assert_eq!(now.tries, 0, "the take's try is given back");
    assert_eq!(courier.asked.load(Ordering::SeqCst), 0, "nothing typed");
}

const NO_DELETE: &str = "CREATE TRIGGER no_delete BEFORE DELETE ON outbox \
    BEGIN SELECT RAISE(ABORT, 'database is locked'); END;";
const NO_UNSURE: &str = "CREATE TRIGGER no_unsure BEFORE UPDATE ON outbox \
    WHEN NEW.state = 'unsure' BEGIN SELECT RAISE(ABORT, 'database is locked'); END;";

#[test]
fn a_failed_delete_after_delivery_is_retried_not_dropped() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.ops.store().run_sql_for_tests(NO_DELETE);
    let mut p = r.postmen.postman(IDLE);
    let next = p.pass(r.at(T0));
    assert!(matches!(next, Next::At(_)), "keeps ticking: {next:?}");
    assert_eq!(r.returns(), 1);
    assert_eq!(state(&r, &row.id), Some((State::Sending, None)));
    assert!(r.room.named("outbox.removed").is_empty(), "not told");
    assert!(matches!(p.pass(r.at(T0 + TICK_MS)), Next::At(_)));
    assert!(r.room.named("outbox.removed").is_empty());
    r.ops.store().run_sql_for_tests("DROP TRIGGER no_delete;");
    p.pass(r.at(T0 + 2 * TICK_MS));
    assert_eq!(r.row(&row.id), None);
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["why"], "sent");
    assert_eq!(r.returns(), 1, "typed once");
}

#[test]
fn a_failed_state_change_after_delivery_is_retried() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", "hi");
    r.ops.store().run_sql_for_tests(NO_UNSURE);
    let mut p = r.postmen.postman(IDLE);
    let next = p.pass(r.at(T0));
    assert!(matches!(next, Next::At(_)), "keeps ticking: {next:?}");
    assert_eq!(state(&r, &row.id), Some((State::Sending, None)));
    r.ops.store().run_sql_for_tests("DROP TRIGGER no_unsure;");
    p.pass(r.at(T0 + TICK_MS));
    assert_eq!(
        state(&r, &row.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    assert_eq!(r.returns(), 1, "typed once");
}

#[test]
fn an_owed_row_is_landed_before_acting_off_ends_the_postman() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.ops.store().run_sql_for_tests(NO_DELETE);
    let mut p = r.postmen.postman(IDLE);
    assert!(matches!(p.pass(r.at(T0)), Next::At(_)));
    assert_eq!(state(&r, &row.id), Some((State::Sending, None)));
    r.ops.store().run_sql_for_tests("DROP TRIGGER no_delete;");
    r.acting_switch().set(false);
    assert_eq!(p.pass(r.at(T0 + TICK_MS)), Next::Done);
    assert_eq!(r.row(&row.id), None);
    let gone = r.room.named("outbox.removed");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["why"], "sent");
    assert_eq!(r.returns(), 1, "typed once");
}

#[test]
fn a_sending_head_left_by_a_dropped_postman_is_settled_when_acting_turns_on() {
    let r = rig();
    let row = r.send("job-idle", "hi");
    r.ops.store().run_sql_for_tests(NO_DELETE);
    let mut dropped = r.postmen.postman(IDLE);
    dropped.pass(r.at(T0));
    r.acting_switch().set(false);
    assert_eq!(dropped.pass(r.at(T0 + TICK_MS)), Next::Done);
    drop(dropped);
    assert_eq!(state(&r, &row.id), Some((State::Sending, None)));
    r.ops.store().run_sql_for_tests("DROP TRIGGER no_delete;");
    r.acting_switch().set(true);
    let mut fresh = r.postmen.postman(IDLE);
    assert_eq!(
        fresh.pass(r.at(T0 + 2 * TICK_MS)),
        Next::At(T0 + 2 * TICK_MS + UNSURE_HOLD_MS),
        "an unsure head waits for its turn to step aside"
    );
    assert_eq!(
        state(&r, &row.id),
        Some((State::Unsure, Some(Why::Restart)))
    );
    assert_eq!(r.returns(), 1, "typed once");
}

#[test]
fn a_slow_read_never_stamps_a_row_with_the_time_before_it() {
    use notchling_engine::sessions::fresh::Ended;
    let r = rig();
    let row = r.send("job-a", "hi");
    r.files.read_as(BUSY, Ended::Unknown);
    r.files.reads_take(&r.clock, 1_500);
    let mut p = r.postmen.postman(BUSY);
    p.pass(r.at(T0));
    let after = r.row(&row.id).unwrap();
    assert_eq!(after.state, State::Waiting);
    assert_eq!(after.updated_ms, T0 + 1_500);
}

const BOX_RULE: &str = "────────────────────────────────────────";

fn box_holding(r: &Rig, text: &str) {
    let held = format!("❯ {text}");
    r.show(&[BOX_RULE, &held, BOX_RULE, "  ? for shortcuts"]);
}

fn pasted_at(writes: &[String], text: &str) -> Vec<(usize, usize)> {
    let keys = [keys::clear_input(), keys::ENTER.to_string()];
    let mut found = Vec::new();
    for (n, write) in writes.iter().enumerate() {
        if !keys.contains(write) {
            found.extend(write.match_indices(text).map(|(at, _)| (n, at)));
        }
    }
    found
}

fn an_ignored_enter_is_tried_again_before_the_next_message(first: &str, next: &str, tried_at: i64) {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let head = r.send("job-idle", first);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(tried_at));
    r.safe.note(IDLE, false);
    let behind = r.send("job-idle", next);
    let tried = r.row(&head.id).unwrap();
    assert_eq!(tried.tries, 1);
    assert_eq!(
        (tried.state, tried.why),
        (State::Waiting, Some(Why::Retry)),
        "the text is still in the box and the transcript has nothing: not taken"
    );
    assert_eq!(tried.next_ms, Some(tried_at + 30_000));
    assert!(r.ops.note(&head.id).is_some(), "the failure is shown");
    assert_eq!(state(&r, &behind.id), Some((State::Queued, None)));
    let first_try = r.writes.lock().unwrap().clone();
    assert_eq!(pasted_at(&first_try, first).len(), 1);
    assert!(pasted_at(&first_try, next).is_empty());
    p.pass(r.at(tried_at + 29_999));
    assert_eq!(r.row(&head.id).unwrap().tries, 1, "not before next_ms");
    assert_eq!(r.writes.lock().unwrap().len(), first_try.len());
    r.deaf.store(false, Ordering::SeqCst);
    box_holding(&r, first);
    let at = tried_at + 30_000;
    for step in 0..=6 {
        p.pass(r.at(at + step * TICK_MS));
    }
    assert_eq!(r.row(&head.id), None, "the retry was taken and proved");
    assert_eq!(r.row(&behind.id), None);
    let writes = r.writes.lock().unwrap()[first_try.len()..].to_vec();
    assert_eq!(
        writes.first().map(String::as_str),
        Some(keys::clear_input().as_str()),
        "its own leftover goes first"
    );
    let again = pasted_at(&writes, first);
    let after = pasted_at(&writes, next);
    assert_eq!((again.len(), after.len()), (1, 1), "each typed once");
    assert!(again[0] < after[0], "the first goes before the second");
}

#[test]
fn a_message_the_restarted_session_ignored_is_tried_again_before_the_next_message() {
    an_ignored_enter_is_tried_again_before_the_next_message("is it right", "and this?", T0);
}

#[test]
fn a_try_the_session_took_but_nothing_proves_still_holds_the_next_message() {
    let r = rig();
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let head = r.send("job-idle", "is it right");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    r.safe.note(IDLE, false);
    let behind = r.send("job-idle", "and this?");
    assert_eq!(r.returns(), 1, "the Enter was taken");
    assert_eq!(
        state(&r, &head.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
    for step in 1..UNSURE_HOLD_MS / 10_000 {
        p.pass(r.at(T0 + step * 10_000));
    }
    assert_eq!(state(&r, &behind.id), Some((State::Queued, None)));
    assert!(!r.typed().contains("and this?"));
    assert_eq!(r.returns(), 1);
    for step in [1_000, 2_000, 5_000, 8_000] {
        p.pass(r.at(T0 + UNSURE_HOLD_MS + step));
    }
    assert_eq!(r.row(&behind.id), None, "typed once the hold ran out");
    assert!(r.typed().contains("and this?"));
    assert_eq!(
        state(&r, &head.id),
        Some((State::Unsure, Some(Why::NotFound)))
    );
}
