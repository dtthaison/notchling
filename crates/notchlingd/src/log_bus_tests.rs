use crate::log_bus::{BusConfig, Enabled, LogBus, Sink, SinkResult};
use crate::log_queue::Stamped;
use notchling_events::kinds::choices::CallerTier;
use notchling_events::kinds::diagnostics::{CallResult, OpCalled};
use notchling_events::kinds::security::{ApiRefused, RefusedWhy};
use notchling_events::scope::Scope;
use notchling_events::spec::{Outcome, Sinks};
use notchling_events::{Actor, ActorTier, Emit, Event, Exe, Hex, Millis, Op, UnwritableWhy, Via};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Shared {
    batches: Mutex<Vec<Vec<Stamped>>>,
    flushes: Mutex<Vec<bool>>,
    stall: AtomicBool,
    fail: AtomicBool,
}

impl Shared {
    fn written(&self) -> Vec<Stamped> {
        self.batches
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .cloned()
            .collect()
    }

    fn names(&self) -> Vec<&'static str> {
        self.written().iter().map(|s| s.event.spec.name).collect()
    }
}

struct FakeSink {
    shared: Arc<Shared>,
}

impl Sink for FakeSink {
    fn sinks(&self) -> Sinks {
        Sinks::AUDIT
    }

    fn write(&mut self, batch: &[Stamped], _audit_n: &[Option<u64>]) -> SinkResult {
        while self.shared.stall.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        if self.shared.fail.load(Ordering::SeqCst) {
            return SinkResult {
                audit_n: vec![None; batch.len()],
                failed: true,
            };
        }
        self.shared.batches.lock().unwrap().push(batch.to_vec());
        SinkResult {
            audit_n: (0..batch.len() as u64).map(|n| Some(n + 100)).collect(),
            failed: false,
        }
    }

    fn flush(&mut self, full: bool) -> std::io::Result<()> {
        self.shared.flushes.lock().unwrap().push(full);
        Ok(())
    }

    fn tick(&mut self, _now_ms: i64) -> Vec<Event> {
        Vec::new()
    }
}

fn config() -> BusConfig {
    let mut cfg = BusConfig::daemon(Hex::new("abcdef0123456789").unwrap());
    cfg.batch_ms = 10;
    cfg.commit_wait = Duration::from_millis(400);
    cfg.seq_start = 41;
    cfg
}

fn start_with(cfg: BusConfig, enabled: Enabled) -> (Arc<LogBus>, Arc<Shared>) {
    let shared = Arc::new(Shared::default());
    let sink = FakeSink {
        shared: shared.clone(),
    };
    let bus = LogBus::start(cfg, vec![Box::new(sink)], enabled);
    (bus, shared)
}

fn start() -> (Arc<LogBus>, Arc<Shared>) {
    start_with(config(), Arc::new(|_| true))
}

fn toggle_event(ts: i64) -> Event {
    Event::new(
        &OpCalled {
            op: Op::named("session.reply"),
            tier: CallerTier::App,
            result: CallResult::Ok,
            code: None,
            ms: Millis(1),
        },
        Outcome::Ok,
        ts,
    )
}

fn always_event(op: &'static str) -> Event {
    Event::new(
        &ApiRefused {
            op: Op::named(op),
            exe: Some(Exe::named("codex")),
            tier: CallerTier::Agent,
            why: RefusedWhy::Forbidden,
            switch: None,
        },
        Outcome::Refused,
        5,
    )
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn push_stamps_seq_host_boot_and_scope() {
    let (bus, shared) = start();
    let actor = Actor {
        tier: ActorTier::Agent,
        via: Some(Via::Cli),
        exe: Some(Exe::named("claude")),
        device: None,
        session: None,
    };
    let scope = Scope {
        op_id: Hex::new("00112233"),
        actor: Some(actor.clone()),
    };
    scope.run(|| {
        bus.push(always_event("a"));
        bus.push(always_event("b"));
    });
    bus.push(always_event("c"));
    wait_for("three events", || shared.written().len() == 3);
    let written = shared.written();
    let seqs: Vec<u64> = written.iter().map(|s| s.env.seq).collect();
    assert_eq!(seqs, vec![41, 42, 43]);
    assert_eq!(written[0].env.host.as_str(), "abcdef0123456789");
    assert_eq!(written[0].env.boot.as_str(), written[2].env.boot.as_str());
    assert_eq!(written[0].env.pid, std::process::id());
    assert_eq!(written[0].env.op_id.as_ref().unwrap().as_str(), "00112233");
    assert_eq!(written[1].env.actor.as_ref(), Some(&actor));
    assert!(written[2].env.actor.is_none());
    assert!(written[2].env.op_id.is_none());
    bus.close(Duration::from_secs(2));
}

#[test]
fn push_never_blocks_when_writer_stalls() {
    let (bus, shared) = start();
    shared.stall.store(true, Ordering::SeqCst);
    bus.push(toggle_event(0));
    wait_for("the writer to take the first batch", || {
        bus.stats().pushed == 1 && lock_is_empty(&bus)
    });
    let started = Instant::now();
    for i in 0..20_000 {
        bus.push(toggle_event(i));
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
    let stats = bus.stats();
    assert_eq!(stats.dropped_toggle, 20_000 - 16_384);
    shared.stall.store(false, Ordering::SeqCst);
    bus.close(Duration::from_secs(5));
}

fn lock_is_empty(bus: &LogBus) -> bool {
    bus.stats().overflow_bytes == 0 && bus.queued() == 0
}

#[test]
fn always_events_overflow_not_drop() {
    let (bus, shared) = start();
    shared.stall.store(true, Ordering::SeqCst);
    for _ in 0..5_000 {
        bus.push(always_event("x"));
    }
    let stats = bus.stats();
    assert_eq!(stats.dropped_always, 0);
    assert!(stats.overflow_bytes > 0);
    shared.stall.store(false, Ordering::SeqCst);
    bus.close(Duration::from_secs(10));
    assert_eq!(
        shared
            .written()
            .iter()
            .filter(|s| s.event.spec.name == "api.refused")
            .count(),
        5_000
    );
}

#[test]
fn always_events_past_the_byte_cap_are_dropped_and_counted() {
    let mut cfg = config();
    cfg.overflow_bytes = 4_000;
    let (bus, shared) = start_with(cfg, Arc::new(|_| true));
    shared.stall.store(true, Ordering::SeqCst);
    for _ in 0..50 {
        bus.push(always_event("x"));
    }
    assert!(bus.stats().dropped_always > 0);
    shared.stall.store(false, Ordering::SeqCst);
    bus.close(Duration::from_secs(5));
    let names = shared.names();
    assert!(names.contains(&"log.dropped"), "{names:?}");
}

#[test]
fn commit_waits_for_flush() {
    let (bus, shared) = start();
    let done = bus.commit(always_event("q")).unwrap();
    assert!(done.seq >= 41);
    assert_eq!(done.audit_n, Some(100));
    assert!(shared.flushes.lock().unwrap().contains(&true));
    bus.close(Duration::from_secs(2));
}

#[test]
fn commit_fails_on_write_error_and_releases_nothing() {
    let (bus, shared) = start();
    shared.fail.store(true, Ordering::SeqCst);
    let actor = Actor {
        tier: ActorTier::App,
        via: Some(Via::App),
        exe: Some(Exe::named("notchling-app")),
        device: None,
        session: None,
    };
    let result = Scope {
        op_id: None,
        actor: Some(actor),
    }
    .run(|| bus.commit(always_event("settings.set")));
    assert_eq!(result.unwrap_err().why, UnwritableWhy::WriteFailed);
    assert!(shared.written().is_empty());
    assert_eq!(bus.stats().commits_failed, 1);
    shared.fail.store(false, Ordering::SeqCst);
    bus.push(always_event("later"));
    wait_for("the recovery events", || {
        let names = shared.names();
        names.contains(&"log.dropped") && names.iter().filter(|n| **n == "api.refused").count() == 2
    });
    let written = shared.written();
    let refused: Vec<&Stamped> = written
        .iter()
        .filter(|s| s.event.spec.name == "api.refused")
        .collect();
    let recovered = refused
        .iter()
        .find(|s| format!("{:?}", s.event.fields).contains("audit_unwritable"))
        .expect("audit_unwritable refusal");
    assert_eq!(
        recovered.env.actor.as_ref().unwrap().tier,
        ActorTier::App,
        "the refusal keeps who asked"
    );
    let dropped = written
        .iter()
        .find(|s| s.event.spec.name == "log.dropped")
        .unwrap();
    assert!(format!("{:?}", dropped.event.fields).contains("write_failed"));
    bus.close(Duration::from_secs(2));
}

#[test]
fn commit_times_out_after_the_wait() {
    let mut cfg = config();
    cfg.commit_wait = Duration::from_millis(100);
    let (bus, shared) = start_with(cfg, Arc::new(|_| true));
    shared.stall.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let result = bus.commit(always_event("slow"));
    assert_eq!(result.unwrap_err().why, UnwritableWhy::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    shared.stall.store(false, Ordering::SeqCst);
    bus.close(Duration::from_secs(2));
}

#[test]
fn commit_after_close_is_closed() {
    let (bus, _shared) = start();
    bus.close(Duration::from_secs(2));
    assert_eq!(
        bus.commit(always_event("late")).unwrap_err().why,
        UnwritableWhy::Closed
    );
}

#[test]
fn disabled_kind_is_not_written() {
    let enabled: Enabled = Arc::new(|spec| spec.name != "op.called");
    let (bus, shared) = start_with(config(), enabled);
    bus.push(toggle_event(1));
    bus.push(always_event("kept"));
    bus.close(Duration::from_secs(2));
    assert_eq!(shared.names(), vec!["api.refused"]);
}

#[test]
fn an_always_kind_ignores_the_enabled_switch() {
    let enabled: Enabled = Arc::new(|_| false);
    let (bus, shared) = start_with(config(), enabled);
    bus.push(always_event("kept"));
    bus.push(toggle_event(1));
    bus.close(Duration::from_secs(2));
    assert_eq!(shared.names(), vec!["api.refused"]);
}

#[test]
fn close_drains_everything() {
    let (bus, shared) = start();
    for i in 0..300 {
        bus.push(toggle_event(i));
        bus.push(always_event("x"));
    }
    bus.close(Duration::from_secs(5));
    assert_eq!(shared.written().len(), 600);
    assert!(shared.flushes.lock().unwrap().contains(&true));
}

#[test]
fn an_oversize_event_is_counted_not_written() {
    let (bus, shared) = start();
    let mut event = toggle_event(1);
    let huge = vec![(
        "op",
        notchling_events::field::FieldValue::Token("x".repeat(5_000).into()),
    )];
    event.fields = notchling_events::Fields::new(huge);
    bus.push(event);
    bus.close(Duration::from_secs(2));
    assert_eq!(bus.stats().dropped_invalid, 1);
    assert!(shared.names().iter().all(|n| *n != "op.called"));
}
