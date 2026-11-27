use crate::outbox_courier::{Courier, NOTHING};
use crate::outbox_postman::Postmen;
use crate::outbox_testkit::{rig_with, Options, Rig, IDLE, T0};
use notchling_engine::outbox::{
    Anchor, Facts, Job, Row, State, Tried, TryEnd, Verdict, Why, DONE_STOPS_AFTER_TRIES,
    WAKE_FORGIVEN_TRIES,
};
use notchling_engine::reply::exec::Failure;
use notchling_engine::transcript::sent::Seen;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Waking {
    fail: Failure,
    fails: usize,
    delivered: AtomicUsize,
}

impl Waking {
    fn new(fail: Failure, fails: usize) -> Arc<Waking> {
        Arc::new(Waking {
            fail,
            fails,
            delivered: AtomicUsize::new(0),
        })
    }

    fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }
}

impl Courier for Waking {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Done,
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: Some(0),
        }
    }

    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        let n = self.delivered.fetch_add(1, Ordering::SeqCst);
        match n < self.fails {
            true => (TryEnd::Failed(self.fail.clone()), NOTHING, None),
            false => (
                TryEnd::Ok,
                Tried {
                    pasted: true,
                    entered: true,
                },
                None,
            ),
        }
    }

    fn close(&self, _session: &str) {}

    fn verifies(&self) -> bool {
        false
    }
}

fn rig_of(courier: &Arc<Waking>) -> Rig {
    rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    })
}

fn due(r: &Rig, id: &str) -> i64 {
    r.row(id).unwrap().next_ms.unwrap()
}

#[test]
fn a_done_job_that_is_slow_to_wake_is_tried_again_and_never_stopped() {
    let slow = WAKE_FORGIVEN_TRIES as usize;
    let courier = Waking::new(Failure::Timeout, slow);
    let r = rig_of(&courier);
    let row = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for n in 1..=slow {
        p.pass(r.at(at));
        let tried = r.row(&row.id).unwrap();
        assert_eq!(tried.state, State::Waiting, "try {n}");
        at = due(&r, &row.id);
    }
    assert_eq!(courier.delivered(), slow);
    p.pass(r.at(at));
    assert_eq!(courier.delivered(), slow + 1);
    assert!(
        r.row(&row.id).is_none(),
        "the reply went in once the job was up"
    );
}

#[test]
fn a_done_job_whose_tries_end_on_the_wrong_screen_still_stops_after_three() {
    let courier = Waking::new(Failure::NoInput, usize::MAX);
    let r = rig_of(&courier);
    let row = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for _ in 0..DONE_STOPS_AFTER_TRIES {
        p.pass(r.at(at));
        at = due(&r, &row.id);
    }
    p.pass(r.at(at));
    assert_eq!(r.row(&row.id).unwrap().state, State::Stopped);
    assert_eq!(courier.delivered(), DONE_STOPS_AFTER_TRIES as usize);
}

#[test]
fn a_job_that_times_out_every_time_is_stopped_after_the_forgiven_tries_and_three_more() {
    let courier = Waking::new(Failure::Timeout, usize::MAX);
    let r = rig_of(&courier);
    let row = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    let allowed = (WAKE_FORGIVEN_TRIES + DONE_STOPS_AFTER_TRIES) as usize;
    for n in 1..=allowed {
        p.pass(r.at(at));
        assert_eq!(r.row(&row.id).unwrap().state, State::Waiting, "try {n}");
        at = due(&r, &row.id);
    }
    p.pass(r.at(at));
    assert_eq!(r.row(&row.id).unwrap().state, State::Stopped);
    assert_eq!(courier.delivered(), allowed);
}

#[test]
fn a_row_with_many_earlier_tries_still_gets_its_forgiven_wakes() {
    let slow = WAKE_FORGIVEN_TRIES as usize;
    let courier = Waking::new(Failure::Timeout, slow);
    let r = rig_of(&courier);
    let row = r.send("job-idle", "is the check green now?");
    r.ops.store().run_sql_for_tests(&format!(
        "UPDATE outbox SET tries = 9 WHERE id = '{}'",
        row.id
    ));
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for n in 1..=slow {
        p.pass(r.at(at));
        assert_eq!(r.row(&row.id).unwrap().state, State::Waiting, "try {n}");
        at = due(&r, &row.id);
    }
    p.pass(r.at(at));
    assert!(
        r.row(&row.id).is_none(),
        "the reply went in once the job was up"
    );
}

struct Blocking {
    entered: AtomicBool,
    stopped: AtomicBool,
}

impl Courier for Blocking {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: Job::Done,
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: Some(0),
        }
    }

    fn deliver(&self, _row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        self.entered.store(true, Ordering::SeqCst);
        let began = Instant::now();
        while !self.stopped.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(5));
        }
        (TryEnd::Failed(Failure::Timeout), NOTHING, None)
    }

    fn close(&self, _session: &str) {}

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn verifies(&self) -> bool {
        false
    }
}

#[test]
fn stop_returns_at_once_while_a_wake_is_still_being_waited_on() {
    let courier = Arc::new(Blocking {
        entered: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
    });
    let r = rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    });
    r.threaded();
    r.send("job-idle", "is the check green now?");
    let began = Instant::now();
    while !courier.entered.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(courier.entered.load(Ordering::SeqCst), "the wake started");
    let stopping = Instant::now();
    r.postmen.stop();
    assert!(
        stopping.elapsed() < Duration::from_secs(2),
        "stop took {:?}",
        stopping.elapsed()
    );
}

#[test]
fn a_stopped_courier_gives_up_the_wake_with_no_key_and_says_so() {
    let dir = std::env::temp_dir().join(format!("nl-wake-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("reply-screen.txt");
    let r = rig_with(Options {
        reply_screen: Some(file.clone()),
        ..Options::default()
    });
    r.starting.store(600_000, Ordering::SeqCst);
    r.courier.stop();
    r.send("job-idle", "is the check green now?");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert!(r.writes.lock().unwrap().is_empty());
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(kept.contains("stopping"), "{kept}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stop_during_a_wake_puts_the_row_back_untried_and_does_not_back_off() {
    let courier = Arc::new(Blocking {
        entered: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
    });
    let r = rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    });
    r.threaded();
    let row = r.send("job-idle", "is the check green now?");
    let began = Instant::now();
    while !courier.entered.load(Ordering::SeqCst) && began.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(courier.entered.load(Ordering::SeqCst), "the wake started");
    r.postmen.stop();
    let left = r.row(&row.id).unwrap();
    assert_eq!(
        (left.state, left.tries, left.next_ms, left.done_from),
        (State::Queued, 0, None, Some(0)),
        "{left:?}"
    );
}

struct Stops {
    job: Job,
    end: TryEnd,
    tried: Tried,
    stop_on: usize,
    postmen: Mutex<Option<Postmen>>,
    delivered: AtomicUsize,
    verifies: bool,
    typed: Mutex<Vec<String>>,
    before_take: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

const TYPED: Tried = Tried {
    pasted: true,
    entered: true,
};

fn stops(job: Job, (end, tried): (TryEnd, Tried), stop_on: usize, verifies: bool) -> Arc<Stops> {
    Arc::new(Stops {
        job,
        end,
        tried,
        stop_on,
        postmen: Mutex::new(None),
        delivered: AtomicUsize::new(0),
        verifies,
        typed: Mutex::new(Vec::new()),
        before_take: Mutex::new(None),
    })
}

fn idle_job() -> Job {
    Job::Live {
        status: Some("idle".to_string()),
    }
}

impl Stops {
    fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }

    fn typed(&self) -> Vec<String> {
        self.typed.lock().unwrap().clone()
    }

    fn race(&self) {
        if let Some(hook) = self.before_take.lock().unwrap().take() {
            hook();
        }
    }
}

impl Courier for Stops {
    fn facts(&self, _row: &Row, now_ms: i64) -> Facts {
        Facts {
            now_ms,
            job: self.job.clone(),
            dialog_on_screen: false,
            queue_waiting: 0,
            safe: false,
            turn_over_since_ms: Some(0),
        }
    }

    fn deliver(&self, row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        self.typed.lock().unwrap().push(row.text.clone());
        let n = self.delivered.fetch_add(1, Ordering::SeqCst) + 1;
        if n < self.stop_on {
            return (self.end.clone(), self.tried, None);
        }
        if let Some(postmen) = self.postmen.lock().unwrap().take() {
            postmen.stop();
        }
        (TryEnd::Failed(Failure::Timeout), NOTHING, None)
    }

    fn close(&self, _session: &str) {}

    fn verifies(&self) -> bool {
        self.verifies
    }

    fn text_left(&self, _row: &Row) -> Option<bool> {
        self.race();
        Some(true)
    }

    fn signed_out(&self, _row: &Row) -> bool {
        self.race();
        false
    }
}

fn rig_stopped_by(courier: &Arc<Stops>) -> Rig {
    let r = rig_with(Options {
        courier: Some(courier.clone()),
        ..Options::default()
    });
    *courier.postmen.lock().unwrap() = Some(r.postmen.clone());
    r
}

#[test]
fn a_stop_during_a_wake_keeps_the_done_episodes_count_across_a_restart() {
    let timeout = (TryEnd::Failed(Failure::Timeout), NOTHING);
    let before = (WAKE_FORGIVEN_TRIES + DONE_STOPS_AFTER_TRIES - 1) as usize;
    let courier = stops(Job::Done, timeout.clone(), before + 1, false);
    let r = rig_stopped_by(&courier);
    let row = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    let mut at = T0;
    for n in 1..=before {
        p.pass(r.at(at));
        assert_eq!(r.row(&row.id).unwrap().state, State::Waiting, "try {n}");
        at = due(&r, &row.id);
    }
    p.pass(r.at(at));
    assert_eq!(courier.delivered(), before + 1);
    let left = r.row(&row.id).unwrap();
    assert_eq!(
        (left.tries, left.next_ms),
        (before as u32, None),
        "{left:?}"
    );

    let after = stops(Job::Done, timeout, usize::MAX, false);
    let mut p = r.restarted_with(after.clone()).postman(IDLE);
    p.pass(r.at(at));
    assert_eq!(after.delivered(), 1, "the cut try is tried again");
    p.pass(r.at(due(&r, &row.id)));
    assert_eq!(
        (r.row(&row.id).unwrap().state, after.delivered()),
        (State::Stopped, 1),
        "the episode had one counted try left"
    );
}

#[test]
fn a_stop_during_a_retry_still_finds_the_first_try_landed_after_a_restart() {
    let not_sent = (TryEnd::Failed(Failure::NotSent), TYPED);
    let courier = stops(idle_job(), not_sent.clone(), 2, true);
    let r = rig_stopped_by(&courier);
    let first = Seen {
        taken: 4,
        enqueued: 2,
    };
    *r.verifier.seen.lock().unwrap() = Some(first);
    *r.verifier.anchor.lock().unwrap() = Some(Anchor {
        file: Some("/tmp/none/idle.jsonl".to_string()),
        slack_ms: 1_500,
    });
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let row = r.send("job-idle", "is the check green now?");
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    let tried = r.row(&row.id).unwrap();
    assert_eq!(
        (tried.state, tried.why, tried.unsent, tried.seen_before),
        (State::Waiting, Some(Why::Retry), true, Some(4)),
        "{tried:?}"
    );
    assert!(tried.anchor.is_some() && tried.attempt_ms.is_some());
    *r.verifier.seen.lock().unwrap() = Some(Seen {
        taken: 9,
        enqueued: 3,
    });
    *r.verifier.anchor.lock().unwrap() = None;
    let at = due(&r, &row.id);
    p.pass(r.at(at));
    assert_eq!(courier.delivered(), 2, "the stop cut try 2's wake");
    let left = r.row(&row.id).unwrap();
    let expected = Row {
        next_ms: None,
        updated_ms: left.updated_ms,
        ..tried.clone()
    };
    assert_eq!(left, expected, "put back as it was after try 1");

    r.verifier.late.store(true, Ordering::SeqCst);
    let after = stops(idle_job(), not_sent, usize::MAX, true);
    let mut p = r.restarted_with(after.clone()).postman(IDLE);
    p.pass(r.at(at));
    assert_eq!(after.delivered(), 0, "try 1 landed, nothing is typed again");
    assert!(r.row(&row.id).is_none(), "the reply counts as sent");
}

#[test]
fn a_row_behind_a_row_left_unsent_is_not_joined_into_its_retry() {
    let not_sent = (TryEnd::Failed(Failure::NotSent), TYPED);
    let courier = stops(idle_job(), not_sent.clone(), 2, true);
    let r = rig_stopped_by(&courier);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let first = "is the check green now?";
    let head = r.send("job-idle", first);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(r.row(&head.id).unwrap().unsent);
    let next = "and the lint?";
    let behind = r.send("job-idle", next);
    let at = due(&r, &head.id);
    p.pass(r.at(at));
    assert_eq!(
        courier.typed(),
        vec![first, first],
        "the retry carries its own text"
    );
    assert!(r.row(&behind.id).is_some(), "the row behind waits its turn");

    r.verifier.late.store(true, Ordering::SeqCst);
    let after = stops(idle_job(), not_sent, usize::MAX, true);
    let mut p = r.restarted_with(after.clone()).postman(IDLE);
    p.pass(r.at(at));
    p.pass(r.at(at));
    assert!(r.row(&head.id).is_none(), "try 1 landed");
    assert_eq!(
        after.typed(),
        vec![next],
        "the row behind is typed on its own"
    );
}

#[test]
fn an_edit_between_the_gate_and_the_take_is_not_undone_by_a_stop() {
    let not_sent = (TryEnd::Failed(Failure::NotSent), TYPED);
    let courier = stops(idle_job(), not_sent, 2, true);
    let r = rig_stopped_by(&courier);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let text = "is the check green now?";
    let row = r.send("job-idle", text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    assert!(r.row(&row.id).unwrap().unsent);
    let at = due(&r, &row.id);
    let (ops, id) = (r.ops.clone(), row.id.clone());
    *courier.before_take.lock().unwrap() = Some(Box::new(move || {
        ops.store().edit(&id, text, at).unwrap();
    }));
    p.pass(r.at(at));
    let edited = r.row(&row.id).unwrap();
    assert_eq!(
        (edited.state, edited.why, edited.tries, edited.unsent),
        (State::Queued, None, 0, false),
        "{edited:?}"
    );
}

#[path = "outbox_take_tests.rs"]
mod take_tests;
