use crate::log_audit::{AuditSink, SealState};
use crate::log_audit_tests::{called, refused, rig, with_history, Rig, KEY, T0};
use crate::log_bus::Sink;
use crate::log_checks_tests::wire;
use crate::log_ops::LogOps;
use crate::op_group::OpGroup;
use notchling_engine::logs::{Checkpoint, KeyStore, MemoryKeyStore, UserDeleted};
use notchling_events::audit_line::UnsealedWhy;
use notchling_events::Salt;
use notchling_local_api::{ops, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const QUICK: Duration = Duration::from_secs(2);

struct Release(Arc<MemoryKeyStore>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn within<T: Send + 'static>(
    wait: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(wait).ok()
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < Duration::from_secs(10), "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn built_within(rig: &Rig, wait: Duration, key_wait: Duration) -> Option<AuditSink> {
    let (dir, keys, info) = (rig.dir(), rig.keys.clone(), rig.info(key_wait));
    within(wait, move || {
        AuditSink::new(dir, keys, info, Vec::new(), Salt([3; 32]))
    })
}

fn head(n: u64) -> Checkpoint {
    Checkpoint {
        n,
        seal: "00".repeat(32),
        month: 0,
        settings_hash: Some("settings".into()),
        prune_anchor: Some("anchor".into()),
        user_deleted: UserDeleted {
            months: vec![202_601],
            at: vec![5],
            count: 3,
        },
        at: 1,
    }
}

fn caller() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn building_the_sink_does_not_wait_on_the_key_store() {
    let rig = rig("kc-new");
    rig.keys.set_key(Some(KEY));
    rig.keys.write_head(&head(1)).unwrap();
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    assert!(
        built_within(&rig, QUICK, Duration::from_secs(2)).is_some(),
        "AuditSink::new waited on the key store"
    );
}

#[test]
fn lines_written_while_the_key_store_is_blocked_are_unsealed_and_the_stretch_is_named_later() {
    let rig = rig("kc-late");
    rig.keys.set_key(Some(KEY));
    rig.keys.write_head(&head(1)).unwrap();
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    let mut sink = built_within(&rig, QUICK, Duration::from_millis(100)).expect("a sink at once");
    let began = Instant::now();
    sink.write(
        &[rig.stamp(called(T0)), rig.stamp(called(T0 + 1))],
        &[None, None],
    );
    assert!(began.elapsed() < QUICK, "a write waited on the key store");
    assert_eq!(sink.sealing(), SealState::Unsealed(UnsealedWhy::KeyWaiting));
    assert!(sink.flush(true).is_ok());
    assert!(rig
        .lines()
        .iter()
        .any(|l| l["unsealed"] == "key_waiting" && l["kind"] == "op.called"));

    rig.keys.release();
    until("the key arrives", || {
        sink.tick(T0 + 5);
        sink.sealing() == SealState::Sealed
    });
    assert!(rig.lines().iter().any(|l| l["kind"] == "audit.checked"
        && l["f"]["result"] == "resumed"
        && l["f"]["from_n"] == 1));

    rig.clock
        .store(T0 + 60_000, std::sync::atomic::Ordering::SeqCst);
    sink.write(&[rig.stamp(refused(T0 + 60_000))], &[None]);
    sink.flush(true).unwrap();
    until("a checkpoint that keeps what the old one carried", || {
        rig.keys.read_head().is_some_and(|h| {
            h.n > 1 && h.prune_anchor.as_deref() == Some("anchor") && h.user_deleted.count == 3
        })
    });
}

#[test]
fn a_blocked_head_write_never_holds_a_flush_or_the_lock() {
    let rig = rig("kc-head");
    let audit = Arc::new(Mutex::new(rig.sink()));
    audit
        .lock()
        .unwrap()
        .write(&[rig.stamp(called(T0))], &[None]);
    assert_eq!(audit.lock().unwrap().sealing(), SealState::Sealed);
    audit.lock().unwrap().flush(true).unwrap();
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    let twice = {
        let (audit, stamps) = (
            audit.clone(),
            vec![
                rig.stamp(refused(T0 + 20_000)),
                rig.stamp(refused(T0 + 40_000)),
            ],
        );
        let clock = rig.clock.clone();
        within(QUICK, move || {
            for (i, stamped) in stamps.into_iter().enumerate() {
                clock.store(
                    T0 + 20_000 * (i as i64 + 1),
                    std::sync::atomic::Ordering::SeqCst,
                );
                let mut sink = audit.lock().unwrap();
                sink.write(&[stamped], &[None]);
                sink.flush(true).unwrap();
            }
        })
    };
    assert!(twice.is_some(), "a flush waited on the key store");
    assert!(
        audit.try_lock().is_ok(),
        "the audit lock was held across the key store"
    );
    rig.keys.release();
    let last = audit.lock().unwrap().head().0;
    until("the newest checkpoint is written", || {
        rig.keys.read_head().is_some_and(|h| h.n == last)
    });
}

#[test]
fn a_checkpoint_waiting_on_the_key_store_is_finished_with_a_bounded_wait() {
    let rig = rig("kc-finish");
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(refused(T0))], &[None]);
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    sink.flush(true).unwrap();
    let began = Instant::now();
    assert!(!sink.head_writer().finish(Duration::from_millis(200)));
    assert!(began.elapsed() < QUICK);
    rig.keys.release();
    assert!(sink.head_writer().finish(Duration::from_secs(5)));
    assert_eq!(rig.keys.read_head().unwrap().n, sink.head().0);
}

#[test]
fn a_chain_start_waits_for_the_key_store_outside_the_audit_lock() {
    let rig = rig("kc-chain");
    with_history(&rig);
    let wired = wire(rig);
    wired.write(vec![called(T0 + 1)]);
    let ops = Arc::new(LogOps::new(wired.audit.clone(), wired.checker.clone()));
    wired.rig.keys.hold();
    let _release = Release(wired.rig.keys.clone());
    let (answer, answered) = mpsc::channel();
    {
        let ops = ops.clone();
        std::thread::spawn(move || {
            let _ =
                answer.send(ops.call(&caller(), ops::LOG_CHAIN_START, &json!({"confirm": true})));
        });
    }
    until("the chain start is inside the key store", || {
        wired.rig.keys.waiting() > 0
    });
    assert!(
        within(QUICK, {
            let audit = wired.audit.clone();
            move || audit.lock().unwrap().sealing()
        })
        .is_some(),
        "the audit lock was held across the key store"
    );
    wired.rig.keys.release();
    let reply: Value = answered
        .recv_timeout(Duration::from_secs(10))
        .expect("an answer once the key store answers")
        .unwrap();
    assert!(reply["chain_started_n"].is_u64(), "{reply}");
}

#[test]
fn a_chain_start_before_the_key_is_read_is_refused_not_waited_for() {
    let rig = rig("kc-chain-early");
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    let sink = built_within(&rig, QUICK, Duration::from_secs(30)).expect("a sink at once");
    let audit = Arc::new(Mutex::new(sink));
    let wired = wire_over(audit.clone(), &rig);
    let ops = LogOps::new(audit, wired);
    let answer = within(QUICK, move || {
        ops.call(&caller(), ops::LOG_CHAIN_START, &json!({"confirm": true}))
    });
    assert!(
        matches!(answer, Some(Err(_))),
        "a chain start waited on a blocked key store: {answer:?}"
    );
}

#[test]
fn verifying_does_not_wait_on_the_key_store() {
    let wired = wire(rig("kc-verify"));
    wired.write(vec![called(T0), refused(T0 + 1)]);
    wired.audit.lock().unwrap().flush(true).unwrap();
    wired.rig.keys.hold();
    let _release = Release(wired.rig.keys.clone());
    let checker = wired.checker.clone();
    let report = within(QUICK, move || checker.run(true).result);
    assert_eq!(report, Some("intact"), "log.verify waited on the key store");
}

fn wire_over(audit: Arc<Mutex<AuditSink>>, rig: &Rig) -> Arc<crate::log_checks::Checker> {
    use crate::log_checks_tests::FakeWarnings;
    use notchling_events::MemorySink;
    let clock = rig.clock.clone();
    crate::log_checks::Checker::new(
        rig.dir(),
        audit,
        Arc::new(MemorySink::new()),
        Arc::new(|_, _, _| {}),
        Arc::new(FakeWarnings::default()),
        Arc::new(move || clock.load(std::sync::atomic::Ordering::SeqCst)),
    )
}

#[test]
fn the_not_sealed_notice_goes_when_the_key_arrives_late() {
    let rig = rig("kc-notice");
    rig.keys.set_key(Some(KEY));
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    let sink = built_within(&rig, QUICK, Duration::from_millis(50)).expect("a sink at once");
    let audit = Arc::new(Mutex::new(sink));
    let warnings = Arc::new(crate::log_checks_tests::FakeWarnings::default());
    let checker = {
        let clock = rig.clock.clone();
        crate::log_checks::Checker::new(
            rig.dir(),
            audit.clone(),
            Arc::new(notchling_events::MemorySink::new()),
            Arc::new(|_, _, _| {}),
            warnings.clone(),
            Arc::new(move || clock.load(std::sync::atomic::Ordering::SeqCst)),
        )
    };
    let _schedule = checker.spawn_schedule();
    let ticker = {
        let (audit, stop) = (
            audit.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                audit.lock().unwrap().tick(T0);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        (stop, handle)
    };
    until("the check says the log is not sealed", || {
        !warnings.raised.lock().unwrap().is_empty()
    });
    rig.keys.release();
    until("the notice is withdrawn once sealed", || {
        !warnings.resolved.lock().unwrap().is_empty()
    });
    ticker.0.store(true, std::sync::atomic::Ordering::SeqCst);
    ticker.1.join().unwrap();
}

#[test]
fn a_prune_keeps_the_month_when_its_anchor_cannot_be_stored_in_time() {
    use crate::log_retention::PruneError;
    use notchling_events::kinds::choices::PruneReason;
    let rig = rig("kc-prune");
    let mut sink = rig.sink();
    let later = T0 + 40 * 86_400_000;
    sink.write(
        &[rig.stamp(called(T0)), rig.stamp(called(later))],
        &[None, None],
    );
    rig.clock.store(later, std::sync::atomic::Ordering::SeqCst);
    let old = notchling_engine::logs::month_of(T0);
    rig.keys.hold();
    let _release = Release(rig.keys.clone());
    let began = Instant::now();
    let refused = sink.prune(old, PruneReason::Retention);
    assert!(matches!(refused, Err(PruneError::Io(_))), "{refused:?}");
    assert!(began.elapsed() < QUICK);
    assert!(rig.dir().months().unwrap().contains(&old));
    rig.keys.release();
    assert!(sink.head_writer().finish(Duration::from_secs(5)));
    assert!(sink.prune(old, PruneReason::Retention).is_ok());
    assert!(!rig.dir().months().unwrap().contains(&old));
    assert!(rig.keys.read_head().unwrap().prune_anchor.is_some());
}
