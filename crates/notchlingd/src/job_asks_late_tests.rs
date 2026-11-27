use super::failure_tests::{failing_with, Failing};
use super::slow_tests::{allow, card, settled, wait_closing};
use super::*;
use crate::consent_ops::Msg;
use notchling_events::kinds::choices::{Answerer, ConsentThen};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

struct Probe {
    slow: bool,
    applied: AtomicU32,
    ended: AtomicU32,
    hold_apply: Mutex<Option<Receiver<()>>>,
    apply_entered: Mutex<Sender<()>>,
    hold_denied: Mutex<Option<Receiver<()>>>,
    denied_entered: Mutex<Sender<()>>,
}

struct Rig {
    f: Failing,
    probe: Arc<Probe>,
    apply_entered: Receiver<()>,
    denied_entered: Receiver<()>,
    release_apply: Sender<()>,
    release_denied: Sender<()>,
}

impl Handler for Probe {
    fn apply_failing(
        &self,
        _row: &Consent,
        _by: Option<Answerer>,
        _device: Option<&str>,
    ) -> Result<Value, Failure> {
        self.applied.fetch_add(1, Ordering::SeqCst);
        let hold = self.hold_apply.lock().unwrap().take();
        if let Some(hold) = hold {
            self.apply_entered.lock().unwrap().send(()).unwrap();
            let _ = hold.recv_timeout(Duration::from_secs(20));
        }
        Ok(json!({}))
    }

    fn ended(&self, _row: &Consent, then: ConsentThen, _device: Option<&str>) {
        self.ended.fetch_add(1, Ordering::SeqCst);
        if then == ConsentThen::Denied {
            let hold = self.hold_denied.lock().unwrap().take();
            if let Some(hold) = hold {
                self.denied_entered.lock().unwrap().send(()).unwrap();
                let _ = hold.recv_timeout(Duration::from_secs(20));
            }
        }
    }

    fn slow(&self, _row: &Consent) -> bool {
        self.slow
    }
}

fn rig(tag: &str, slow: bool) -> Rig {
    let (apply_tx, apply_entered) = channel();
    let (apply_release_tx, apply_release) = channel();
    let (denied_tx, denied_entered) = channel();
    let (denied_release_tx, denied_release) = channel();
    let probe = Arc::new(Probe {
        slow,
        applied: AtomicU32::new(0),
        ended: AtomicU32::new(0),
        hold_apply: Mutex::new(Some(apply_release)),
        apply_entered: Mutex::new(apply_tx),
        hold_denied: Mutex::new(Some(denied_release)),
        denied_entered: Mutex::new(denied_tx),
    });
    let f = failing_with(
        tag,
        ApiError::internal("unused"),
        Arc::new(JobBook::in_memory()),
    );
    f.consent.register(DISPATCH, probe.clone());
    Rig {
        f,
        probe,
        apply_entered,
        denied_entered,
        release_apply: apply_release_tx,
        release_denied: denied_release_tx,
    }
}

#[test]
fn close_waits_for_a_late_settle_that_is_still_running() {
    let r = rig("late-tracked", true);
    let first = card(&r.f, "k-first");
    let second = card(&r.f, "k-second");
    allow(&r.f, &first);
    r.apply_entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let consent = r.f.consent.clone();
    let closing = std::thread::spawn(move || consent.close());
    wait_closing(&r.f);
    r.f.consent
        .decide(&second, false, None, "mac", None)
        .unwrap();
    r.denied_entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    r.release_apply.send(()).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !closing.is_finished(),
        "close returned with a late settle running"
    );
    r.release_denied.send(()).unwrap();
    closing.join().unwrap();
    assert_eq!(
        r.f.consent.get_row(&second).unwrap().state.as_str(),
        "denied"
    );
}

#[test]
fn a_row_that_is_not_slow_is_never_applied_once_the_ops_are_closing() {
    let r = rig("late-no-apply", false);
    let late = card(&r.f, "k-late");
    r.f.consent.close();
    allow(&r.f, &late);
    let row = settled(&r.f, &late);
    assert_eq!(row.state.as_str(), "failed", "{row:?}");
    assert_eq!(r.probe.applied.load(Ordering::SeqCst), 0);
}

#[test]
fn a_decision_after_the_wait_ends_the_row_in_the_store_with_no_callbacks() {
    let r = rig("late-store-only", true);
    let late = card(&r.f, "k-late");
    r.f.consent.close();
    allow(&r.f, &late);
    let row = settled(&r.f, &late);
    assert_eq!(row.state.as_str(), "failed", "{row:?}");
    assert_eq!(r.probe.ended.load(Ordering::SeqCst), 0);
    assert_eq!(r.probe.applied.load(Ordering::SeqCst), 0);
}

fn held_applying(r: &Rig, key: &str) -> String {
    let id = card(&r.f, key);
    allow(&r.f, &id);
    r.apply_entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert_eq!(r.f.consent.get_row(&id).unwrap().state.as_str(), "applying");
    id
}

#[test]
fn a_late_settle_ends_a_row_once() {
    let r = rig("late-once", true);
    let id = held_applying(&r, "k-held");
    r.f.consent.settle_late(Msg::Settle(id.clone()));
    r.f.consent.settle_late(Msg::Settle(id.clone()));
    assert_eq!(r.probe.ended.load(Ordering::SeqCst), 1);
    assert_eq!(r.f.consent.get_row(&id).unwrap().state.as_str(), "failed");
    r.release_apply.send(()).unwrap();
}

#[test]
fn a_late_settle_leaves_a_row_that_restart_withdrew_alone() {
    let r = rig("late-restart", true);
    let id = held_applying(&r, "k-held");
    r.f.consent.store.restart(r.f.consent.now()).unwrap();
    assert_eq!(
        r.f.consent.get_row(&id).unwrap().state.as_str(),
        "withdrawn"
    );
    r.f.consent.settle_late(Msg::Settle(id.clone()));
    assert_eq!(r.probe.ended.load(Ordering::SeqCst), 0);
    assert_eq!(
        r.f.consent.get_row(&id).unwrap().state.as_str(),
        "withdrawn"
    );
    r.release_apply.send(()).unwrap();
}
