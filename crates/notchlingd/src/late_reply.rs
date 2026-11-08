//! A job that may outlast the reply to the call that asked for it.
//!
//! A client waits 5 s for a reply (`CALL_TIMEOUT`), and a new Codex session can take longer than
//! that to be named, and `codex delete` longer than that to finish. [`Late::run`] starts the job
//! on its own thread and waits up to a budget for its result; if the job is still going it says so
//! (`None`) and hands the result, when it comes, to a `finish` function instead, which publishes
//! it as an event. The result goes to exactly one of the two: never both, never neither.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

enum Slot<T> {
    Waiting,
    Done(T),
    /// The caller stopped waiting: the job's result is for `finish`.
    Abandoned,
}

struct Shared<T> {
    slot: Mutex<Slot<T>>,
    ready: Condvar,
}

/// Runs `job` on a thread. Returns its result if it came within `wait`; otherwise `None`, and
/// `finish` takes the result when the job ends. A thread that can't start gives `job`'s result
/// as `on_no_thread`.
pub fn run<T, J, F>(wait: Duration, job: J, finish: F, on_no_thread: T) -> Option<T>
where
    T: Send + 'static,
    J: FnOnce() -> T + Send + 'static,
    F: FnOnce(T) + Send + 'static,
{
    let shared = Arc::new(Shared {
        slot: Mutex::new(Slot::Waiting),
        ready: Condvar::new(),
    });
    let worker = shared.clone();
    let started = std::thread::Builder::new()
        .name("late-reply".into())
        .spawn(move || {
            let result = job();
            let mut slot = worker.slot.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*slot, Slot::Abandoned) {
                drop(slot);
                finish(result);
            } else {
                *slot = Slot::Done(result);
                worker.ready.notify_all();
            }
        });
    if started.is_err() {
        return Some(on_no_thread);
    }
    let guard = shared.slot.lock().unwrap_or_else(|e| e.into_inner());
    let (mut slot, _) = shared
        .ready
        .wait_timeout_while(guard, wait, |slot| matches!(slot, Slot::Waiting))
        .unwrap_or_else(|e| e.into_inner());
    match std::mem::replace(&mut *slot, Slot::Abandoned) {
        Slot::Done(result) => Some(result),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[test]
    fn a_job_in_time_gives_its_result_and_finish_never_runs() {
        let (tx, rx) = channel::<u32>();
        let got = run(
            Duration::from_secs(5),
            || 7,
            move |v| tx.send(v).unwrap(),
            0,
        );
        assert_eq!(got, Some(7));
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn a_slow_job_gives_none_at_the_budget_and_its_result_to_finish() {
        let (tx, rx) = channel::<u32>();
        let release = Arc::new(AtomicBool::new(false));
        let gate = release.clone();
        let began = Instant::now();
        let got = run(
            Duration::from_millis(100),
            move || {
                while !gate.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                9
            },
            move |v| tx.send(v).unwrap(),
            0,
        );
        assert_eq!(got, None);
        assert!(
            began.elapsed() < Duration::from_secs(3),
            "it did not wait for the job"
        );
        assert!(rx.try_recv().is_err(), "the job is still going");
        release.store(true, Ordering::SeqCst);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(9));
    }

    #[test]
    fn a_result_is_never_lost_when_job_and_budget_end_together() {
        // The job ends right around the budget: whichever way it falls, exactly one side has it.
        for n in 0..50u64 {
            let (tx, rx) = channel::<u64>();
            let got = run(
                Duration::from_millis(5),
                move || {
                    std::thread::sleep(Duration::from_micros(4_000 + n * 40));
                    n
                },
                move |v| tx.send(v).unwrap(),
                u64::MAX,
            );
            match got {
                Some(v) => {
                    assert_eq!(v, n);
                    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err(), "{n}");
                }
                None => assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(n), "{n}"),
            }
        }
    }
}
