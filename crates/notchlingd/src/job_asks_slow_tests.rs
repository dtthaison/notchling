use super::failure_tests::{failing_hands, Failing};
use super::*;
use crate::consent_testkit::agent_caller;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Gate {
    op: &'static str,
    held: Mutex<Option<Receiver<()>>>,
    entered: Mutex<Sender<()>>,
}

impl Gate {
    fn pause(&self, op: &str) {
        if op != self.op {
            return;
        }
        self.entered.lock().unwrap().send(()).unwrap();
        let release = self.held.lock().unwrap().take();
        if let Some(release) = release {
            let _ = release.recv_timeout(Duration::from_secs(20));
        }
    }
}

impl Hands for Gate {
    fn dispatch(&self, args: &Value) -> Result<Value, ApiError> {
        if args["key"] == "k-slow" {
            self.pause("dispatch");
        }
        Ok(json!({"job": "j", "state": "queued"}))
    }
    fn tail(&self, _job: &JobRow, _from: u64) -> Result<Value, ApiError> {
        self.pause("tail");
        Ok(json!({}))
    }
    fn give_back(&self, _job: &JobRow) -> Result<Value, ApiError> {
        self.pause("return");
        Ok(json!({}))
    }
    fn halt(&self, _job: &JobRow) -> Result<Value, ApiError> {
        self.pause("stop");
        Ok(json!({}))
    }
}

fn gated(tag: &str, op: &'static str) -> (Failing, Receiver<()>, Sender<()>) {
    let (entered_tx, entered) = channel();
    let (release_tx, release_rx) = channel();
    let gate = Arc::new(Gate {
        op,
        held: Mutex::new(Some(release_rx)),
        entered: Mutex::new(entered_tx),
    });
    let f = failing_hands(tag, gate, Arc::new(JobBook::in_memory()));
    (f, entered, release_tx)
}

pub(super) fn card(f: &Failing, key: &str) -> String {
    let reply = f
        .asks
        .call(
            &agent_caller(),
            ops::CLUSTER_DISPATCH,
            &super::tests::args_for("claude", key),
        )
        .unwrap();
    reply["request"]["id"].as_str().unwrap().to_string()
}

fn job_card(f: &Failing, op: &str) -> String {
    let reply = f
        .asks
        .call(&agent_caller(), op, &json!({"id": "j-own"}))
        .unwrap();
    reply["request"]["id"].as_str().unwrap().to_string()
}

pub(super) fn allow(f: &Failing, id: &str) {
    let said = f.consent.decide(id, true, None, "mac", None).unwrap();
    assert_eq!(said["answered"], json!(true));
}

fn wait_applied(f: &Failing, id: &str) {
    let done = f.consent.wait_for(id, Duration::from_secs(5)).unwrap();
    assert_eq!(done.state.as_str(), "applied", "{id}");
}

#[test]
fn a_slow_dispatch_does_not_hold_up_the_next_request() {
    let (f, entered, release) = gated("slow", "dispatch");
    let slow = card(&f, "k-slow");
    let fast = card(&f, "k-fast");
    allow(&f, &slow);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    allow(&f, &fast);
    wait_applied(&f, &fast);
    release.send(()).unwrap();
    wait_applied(&f, &slow);
}

#[test]
fn a_slow_tail_return_or_stop_does_not_hold_up_the_next_request() {
    for (op, held, tag) in [
        (ops::JOBS_TAIL, "tail", "slow-tail"),
        (ops::JOBS_RETURN, "return", "slow-return"),
        (ops::JOBS_STOP, "stop", "slow-stop"),
    ] {
        let (f, entered, release) = gated(tag, held);
        let slow = job_card(&f, op);
        let fast = card(&f, "k-fast");
        allow(&f, &slow);
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        allow(&f, &fast);
        wait_applied(&f, &fast);
        release.send(()).unwrap();
        wait_applied(&f, &slow);
    }
}

#[test]
fn closing_waits_for_an_apply_that_is_still_running_and_leaves_its_row_settled() {
    let (f, entered, release) = gated("close-waits", "dispatch");
    let slow = card(&f, "k-slow");
    allow(&f, &slow);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let consent = f.consent.clone();
    let closing = std::thread::spawn(move || consent.close());
    wait_closing(&f);
    assert!(
        !closing.is_finished(),
        "close returned with an apply in flight"
    );
    release.send(()).unwrap();
    closing.join().unwrap();
    let row = f.consent.get_row(&slow).unwrap();
    assert_eq!(row.state.as_str(), "applied");
}

#[test]
fn closing_does_not_wait_forever_for_an_apply_that_is_stuck() {
    let (f, entered, release) = gated("close-stuck", "dispatch");
    f.consent.applies.join_within(300);
    let slow = card(&f, "k-slow");
    allow(&f, &slow);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    f.consent.close();
    assert!(started.elapsed() < Duration::from_secs(5));
    release.send(()).unwrap();
}

#[test]
fn nothing_is_applied_apart_once_the_ops_are_closed() {
    let (f, _entered, _release) = gated("close-after", "dispatch");
    f.consent.close();
    assert!(!f.consent.applies.spawn(|| {}));
}

pub(super) fn wait_closing(f: &Failing) {
    let started = Instant::now();
    while !(f.consent.applies.closing() && f.consent.queue.lock().unwrap().is_none()) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "close never began"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn settled(f: &Failing, id: &str) -> Consent {
    let started = Instant::now();
    loop {
        let row = f.consent.get_row(id).unwrap();
        if row.state.as_str() != "applying" || started.elapsed() > Duration::from_secs(3) {
            return row;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_allow_that_arrives_while_close_waits_for_an_apply_ends_failed_not_applying() {
    let (f, entered, release) = gated("close-race", "dispatch");
    let slow = card(&f, "k-slow");
    let late = card(&f, "k-late");
    allow(&f, &slow);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let consent = f.consent.clone();
    let closing = std::thread::spawn(move || consent.close());
    wait_closing(&f);
    allow(&f, &late);
    let row = settled(&f, &late);
    assert_eq!(row.state.as_str(), "failed", "{row:?}");
    release.send(()).unwrap();
    closing.join().unwrap();
}

#[test]
fn an_allow_that_arrives_after_close_ends_failed_not_applying() {
    let (f, _entered, _release) = gated("close-then-allow", "dispatch");
    let late = card(&f, "k-late");
    f.consent.close();
    allow(&f, &late);
    let row = settled(&f, &late);
    assert_eq!(row.state.as_str(), "failed", "{row:?}");
}

fn denied_through_the_inbox(f: &Failing, row: &str) -> Result<(), ApiError> {
    use notchling_engine::inbox::{ActionId, State};
    let item = f
        .inbox
        .own_items()
        .into_iter()
        .find(|item| {
            item.state == State::Open && item.open.as_ref().is_some_and(|o| o["consent"] == row)
        })
        .expect("an open inbox item for the row");
    let inbox = f.inbox.clone();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(inbox.decide(&item.id, ActionId::Deny, "mac"));
    });
    rx.recv_timeout(Duration::from_secs(3))
        .expect("the inbox answer came back")
        .map(|_| ())
}

#[test]
fn denying_through_the_inbox_after_close_does_not_deadlock_and_closes_the_row() {
    let (f, _entered, _release) = gated("close-inbox", "dispatch");
    let late = card(&f, "k-late");
    f.consent.close();
    denied_through_the_inbox(&f, &late).unwrap();
    let row = settled(&f, &late);
    assert_eq!(row.state.as_str(), "denied", "{row:?}");
}

#[test]
fn denying_through_the_inbox_while_close_waits_for_an_apply_does_not_deadlock() {
    let (f, entered, release) = gated("close-inbox-wait", "dispatch");
    let slow = card(&f, "k-slow");
    allow(&f, &slow);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let late = card(&f, "k-late");
    let consent = f.consent.clone();
    let closing = std::thread::spawn(move || consent.close());
    wait_closing(&f);
    denied_through_the_inbox(&f, &late).unwrap();
    let row = settled(&f, &late);
    assert_eq!(row.state.as_str(), "denied", "{row:?}");
    release.send(()).unwrap();
    closing.join().unwrap();
}
