use crate::log_queue::{CommitTicket, Queues, Stamped};
use notchling_events::field::FieldValue;
use notchling_events::kinds::choices::{CallerTier, SinkName};
use notchling_events::kinds::security::ApiRefused;
use notchling_events::kinds::security::RefusedWhy;
use notchling_events::kinds::security_log::{DropReason, LogDropped};
use notchling_events::spec::{KindSpec, Lock, Outcome, Proc, Sinks};
use notchling_events::{
    scope, Actor, ActorTier, Committed, Count, Emit, Envelope, Event, Exe, Hex, Op, Unwritable,
    UnwritableWhy, MAX_EVENT_BYTES,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Extra {
    pub stamped: Stamped,
    pub audit_n: Option<u64>,
}

pub struct SinkResult {
    pub audit_n: Vec<Option<u64>>,
    pub failed: bool,
    pub extra: Vec<Extra>,
}

impl SinkResult {
    pub fn written(len: usize) -> SinkResult {
        SinkResult {
            audit_n: vec![None; len],
            failed: false,
            extra: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Ticked {
    pub events: Vec<Event>,
    pub extra: Vec<Extra>,
}

pub trait Sink: Send {
    fn sinks(&self) -> Sinks;
    fn write(&mut self, batch: &[Stamped], audit_n: &[Option<u64>]) -> SinkResult;
    fn flush(&mut self, full: bool) -> std::io::Result<()>;
    fn tick(&mut self, now_ms: i64) -> Ticked;
}

#[derive(Clone)]
pub struct EnvMaker {
    seq: Arc<AtomicU64>,
    host: Hex,
    boot: Hex,
    pid: u32,
}

impl EnvMaker {
    pub fn new(host: Hex, boot: Hex, pid: u32, seq_start: u64) -> EnvMaker {
        EnvMaker {
            seq: Arc::new(AtomicU64::new(seq_start.max(1))),
            host,
            boot,
            pid,
        }
    }

    pub fn make(&self, op_id: Option<Hex>, actor: Option<Actor>) -> Envelope {
        Envelope {
            ts_rx: None,
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            host: self.host.clone(),
            proc: Proc::Daemon,
            pid: self.pid,
            boot: self.boot.clone(),
            op_id,
            actor,
            repeat: None,
            spooled: false,
        }
    }
}

pub type Enabled = Arc<dyn Fn(&KindSpec) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct BusConfig {
    pub toggle_cap: usize,
    pub always_cap: usize,
    pub overflow_bytes: usize,
    pub batch_ms: u64,
    pub batch_max: usize,
    pub commit_wait: Duration,
    pub env: EnvMaker,
    pub on_health: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl BusConfig {
    pub fn daemon(host: Hex) -> BusConfig {
        BusConfig {
            toggle_cap: 16_384,
            always_cap: 4_096,
            overflow_bytes: 8 * 1024 * 1024,
            batch_ms: 250,
            batch_max: 500,
            commit_wait: Duration::from_secs(2),
            env: EnvMaker::new(
                host,
                notchling_events::ids::new_boot(),
                std::process::id(),
                1,
            ),
            on_health: None,
            clock: Arc::new(wall_ms),
        }
    }
}

pub fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusStats {
    pub pushed: u64,
    pub dropped_toggle: u64,
    pub dropped_always: u64,
    pub dropped_invalid: u64,
    pub commits_failed: u64,
    pub overflow_bytes: u64,
}

#[derive(Default)]
struct Counters {
    pushed: AtomicU64,
    dropped_toggle: AtomicU64,
    dropped_always: AtomicU64,
    dropped_invalid: AtomicU64,
    commits_failed: AtomicU64,
}

#[derive(Default)]
struct Reported {
    toggle: u64,
    always: u64,
    invalid: u64,
}

struct Refusal {
    op: Op,
    exe: Option<Exe>,
    tier: CallerTier,
    op_id: Option<Hex>,
    actor: Option<Actor>,
}

#[derive(Default)]
struct Lost {
    failing: bool,
    events: u64,
    refusals: Vec<Refusal>,
}

pub struct LogBus {
    cfg: BusConfig,
    enabled: Enabled,
    state: Mutex<Queues>,
    ready: Condvar,
    counters: Counters,
    done: Mutex<bool>,
    finished: Condvar,
    writer: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_always(spec: &KindSpec) -> bool {
    matches!(spec.lock, Lock::Always(_))
}

fn caller_tier(actor: Option<&Actor>) -> CallerTier {
    match actor.map(|a| a.tier) {
        Some(ActorTier::App) => CallerTier::App,
        _ => CallerTier::Agent,
    }
}

impl LogBus {
    pub fn start(cfg: BusConfig, sinks: Vec<Box<dyn Sink>>, enabled: Enabled) -> Arc<LogBus> {
        let bus = Arc::new(LogBus {
            cfg,
            enabled,
            state: Mutex::new(Queues::default()),
            ready: Condvar::new(),
            counters: Counters::default(),
            done: Mutex::new(false),
            finished: Condvar::new(),
            writer: Mutex::new(None),
        });
        let runner = bus.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-log".into())
            .spawn(move || runner.run(sinks));
        match spawned {
            Ok(handle) => *lock(&bus.writer) = Some(handle),
            Err(_) => *lock(&bus.done) = true,
        }
        bus
    }

    pub fn stats(&self) -> BusStats {
        let queues = lock(&self.state);
        let overflow = if queues.always.len() > self.cfg.always_cap {
            queues.always_bytes as u64
        } else {
            0
        };
        BusStats {
            pushed: self.counters.pushed.load(Ordering::SeqCst),
            dropped_toggle: self.counters.dropped_toggle.load(Ordering::SeqCst),
            dropped_always: self.counters.dropped_always.load(Ordering::SeqCst),
            dropped_invalid: self.counters.dropped_invalid.load(Ordering::SeqCst),
            commits_failed: self.counters.commits_failed.load(Ordering::SeqCst),
            overflow_bytes: overflow,
        }
    }

    pub fn queued(&self) -> usize {
        lock(&self.state).len()
    }

    pub fn close(&self, wait: Duration) {
        lock(&self.state).closed = true;
        self.ready.notify_all();
        let done = lock(&self.done);
        let (done, _) = self
            .finished
            .wait_timeout_while(done, wait, |finished| !*finished)
            .unwrap_or_else(PoisonError::into_inner);
        let finished = *done;
        drop(done);
        if finished {
            if let Some(handle) = lock(&self.writer).take() {
                let _ = handle.join();
            }
        }
    }

    fn enqueue(
        &self,
        event: Event,
        op_id: Option<Hex>,
        actor: Option<Actor>,
        ticket: Option<CommitTicket>,
        internal: bool,
    ) -> Result<(), Unwritable> {
        let spec = event.spec;
        let always = is_always(spec);
        if !always && !(self.enabled)(spec) {
            return Ok(());
        }
        if event.json_len() > MAX_EVENT_BYTES {
            self.counters.dropped_invalid.fetch_add(1, Ordering::SeqCst);
            return Err(Unwritable {
                why: UnwritableWhy::WriteFailed,
            });
        }
        self.counters.pushed.fetch_add(1, Ordering::SeqCst);
        let bytes = event.json_len();
        let mut queues = lock(&self.state);
        if queues.closed && !internal {
            return Err(Unwritable {
                why: UnwritableWhy::Closed,
            });
        }
        if always {
            if queues.always_bytes + bytes > self.cfg.overflow_bytes {
                if spec.name != "log.dropped" {
                    self.counters.dropped_always.fetch_add(1, Ordering::SeqCst);
                }
                return Err(Unwritable {
                    why: UnwritableWhy::Overflow,
                });
            }
            let env = self.cfg.env.make(op_id, actor);
            queues.always_bytes += bytes;
            queues.always.push_back(Stamped {
                env,
                event,
                commit: ticket,
            });
        } else {
            if queues.toggle.len() >= self.cfg.toggle_cap {
                self.counters.dropped_toggle.fetch_add(1, Ordering::SeqCst);
                return Ok(());
            }
            let env = self.cfg.env.make(op_id, actor);
            queues.toggle.push_back(Stamped {
                env,
                event,
                commit: ticket,
            });
        }
        drop(queues);
        self.ready.notify_one();
        Ok(())
    }

    fn take_batch(&self) -> (Vec<Stamped>, bool) {
        let mut queues = lock(&self.state);
        if queues.is_empty() {
            if queues.closed {
                return (Vec::new(), true);
            }
            let (guard, _) = self
                .ready
                .wait_timeout(queues, Duration::from_secs(1))
                .unwrap_or_else(PoisonError::into_inner);
            queues = guard;
            if queues.is_empty() {
                let closed = queues.closed;
                return (Vec::new(), closed);
            }
        }
        let deadline = Instant::now() + Duration::from_millis(self.cfg.batch_ms);
        while !queues.closed && queues.len() < self.cfg.batch_max && !queues.has_commit() {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (guard, _) = self
                .ready
                .wait_timeout(queues, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            queues = guard;
        }
        let batch = queues.take_batch(self.cfg.batch_max);
        (batch, queues.closed)
    }

    fn requeue(&self, event: Event, op_id: Option<Hex>, actor: Option<Actor>) {
        let _ = self.enqueue(event, op_id, actor, None, true);
    }

    fn report_drops(&self, reported: &mut Reported) {
        let toggle = self.counters.dropped_toggle.load(Ordering::SeqCst);
        let always = self.counters.dropped_always.load(Ordering::SeqCst);
        let invalid = self.counters.dropped_invalid.load(Ordering::SeqCst);
        let now = (self.cfg.clock)();
        for (sink, reason, total, seen) in [
            (
                SinkName::Activity,
                DropReason::QueueFull,
                toggle,
                &mut reported.toggle,
            ),
            (
                SinkName::Audit,
                DropReason::QueueFull,
                always,
                &mut reported.always,
            ),
            (
                SinkName::Audit,
                DropReason::Invalid,
                invalid,
                &mut reported.invalid,
            ),
        ] {
            if total > *seen {
                let event = Event::new(
                    &LogDropped {
                        sink,
                        count: Count(total - *seen),
                        reason,
                    },
                    Outcome::Failed,
                    now,
                );
                *seen = total;
                self.requeue(event, None, None);
            }
        }
    }

    fn write_batch(
        &self,
        sinks: &mut [Box<dyn Sink>],
        batch: &[Stamped],
        lost: &mut Lost,
        dirty: &mut bool,
    ) {
        let has_commit = batch.iter().any(|s| s.commit.is_some());
        let has_always = batch.iter().any(|s| is_always(s.event.spec));
        let mut carry: Vec<Option<u64>> = vec![None; batch.len()];
        let mut broken: Vec<bool> = vec![false; sinks.len()];
        let mut extras: Vec<Extra> = Vec::new();
        for (i, sink) in sinks.iter_mut().enumerate() {
            let before = extras.len();
            let mut result = if extras.is_empty() {
                sink.write(batch, &carry)
            } else {
                let mut combined: Vec<Stamped> = extras.iter().map(|e| e.stamped.clone()).collect();
                combined.extend_from_slice(batch);
                let mut audit: Vec<Option<u64>> = extras.iter().map(|e| e.audit_n).collect();
                audit.extend_from_slice(&carry);
                let mut result = sink.write(&combined, &audit);
                let cut = before.min(result.audit_n.len());
                result.audit_n.drain(..cut);
                result
            };
            broken[i] = result.failed;
            if result.audit_n.len() == batch.len() && result.audit_n.iter().any(Option::is_some) {
                carry = std::mem::take(&mut result.audit_n);
            }
            extras.append(&mut result.extra);
        }
        if has_commit || has_always {
            for (i, sink) in sinks.iter_mut().enumerate() {
                if sink.flush(has_commit).is_err() {
                    broken[i] = true;
                }
            }
            *dirty = false;
        } else {
            *dirty = true;
        }
        let masks: Vec<Sinks> = sinks.iter().map(|s| s.sinks()).collect();
        let hurt = |spec: &KindSpec| -> bool {
            masks
                .iter()
                .zip(&broken)
                .any(|(mask, bad)| *bad && spec.sinks.has(*mask))
        };
        let mut any_loss = false;
        for (i, stamped) in batch.iter().enumerate() {
            let failed = hurt(stamped.event.spec);
            if failed && is_always(stamped.event.spec) {
                lost.events += 1;
                any_loss = true;
            }
            let Some(ticket) = &stamped.commit else {
                continue;
            };
            if failed {
                self.counters.commits_failed.fetch_add(1, Ordering::SeqCst);
                lost.refusals.push(refusal_of(stamped));
                ticket.answer(Err(Unwritable {
                    why: UnwritableWhy::WriteFailed,
                }));
            } else {
                ticket.answer(Ok(Committed {
                    seq: stamped.env.seq,
                    audit_n: carry.get(i).copied().flatten(),
                }));
            }
        }
        if any_loss || broken.iter().any(|b| *b) {
            if !lost.failing {
                lost.failing = true;
                self.health(false);
            }
        } else {
            self.report_recovery(lost);
        }
    }

    fn health(&self, ok: bool) {
        if let Some(report) = &self.cfg.on_health {
            report(ok);
        }
    }

    fn report_recovery(&self, lost: &mut Lost) {
        if lost.failing {
            lost.failing = false;
            self.health(true);
        }
        let now = (self.cfg.clock)();
        for refusal in lost.refusals.drain(..) {
            let event = Event::new(
                &ApiRefused {
                    op: refusal.op,
                    exe: refusal.exe,
                    tier: refusal.tier,
                    why: RefusedWhy::AuditUnwritable,
                    switch: None,
                },
                Outcome::Refused,
                now,
            );
            self.requeue(event, refusal.op_id, refusal.actor);
        }
        if lost.events > 0 {
            let event = Event::new(
                &LogDropped {
                    sink: SinkName::Audit,
                    count: Count(lost.events),
                    reason: DropReason::WriteFailed,
                },
                Outcome::Failed,
                now,
            );
            lost.events = 0;
            self.requeue(event, None, None);
        }
    }

    fn tick(&self, sinks: &mut [Box<dyn Sink>], dirty: &mut bool, reported: &mut Reported) {
        let now = (self.cfg.clock)();
        for i in 0..sinks.len() {
            let ticked = sinks[i].tick(now);
            for event in ticked.events {
                self.requeue(event, None, None);
            }
            if !ticked.extra.is_empty() {
                let stamped: Vec<Stamped> =
                    ticked.extra.iter().map(|e| e.stamped.clone()).collect();
                let audit: Vec<Option<u64>> = ticked.extra.iter().map(|e| e.audit_n).collect();
                for later in sinks.iter_mut().skip(i + 1) {
                    let _ = later.write(&stamped, &audit);
                }
            }
        }
        for sink in sinks.iter_mut() {
            if *dirty {
                let _ = sink.flush(false);
            }
        }
        *dirty = false;
        self.report_drops(reported);
    }

    fn run(self: Arc<Self>, mut sinks: Vec<Box<dyn Sink>>) {
        let mut last_tick = Instant::now();
        let mut dirty = false;
        let mut lost = Lost::default();
        let mut reported = Reported::default();
        loop {
            let (batch, closing) = self.take_batch();
            if !batch.is_empty() {
                self.write_batch(&mut sinks, &batch, &mut lost, &mut dirty);
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                last_tick = Instant::now();
                self.tick(&mut sinks, &mut dirty, &mut reported);
            }
            if closing && lock(&self.state).is_empty() {
                self.report_drops(&mut reported);
                if !lock(&self.state).is_empty() {
                    continue;
                }
                for sink in sinks.iter_mut() {
                    let _ = sink.flush(true);
                }
                break;
            }
        }
        *lock(&self.done) = true;
        self.finished.notify_all();
    }
}

fn refusal_of(stamped: &Stamped) -> Refusal {
    let op = match stamped.event.fields.get("op") {
        Some(FieldValue::Name(name)) => Op::named(name),
        _ => Op::parse("", &[]),
    };
    Refusal {
        op,
        exe: stamped.env.actor.as_ref().and_then(|a| a.exe.clone()),
        tier: caller_tier(stamped.env.actor.as_ref()),
        op_id: stamped.env.op_id.clone(),
        actor: stamped.env.actor.clone(),
    }
}

impl Emit for LogBus {
    fn push(&self, event: Event) {
        let current = scope::current();
        let _ = self.enqueue(event, current.op_id, current.actor, None, false);
    }

    fn commit(&self, event: Event) -> Result<Committed, Unwritable> {
        let current = scope::current();
        let ticket = CommitTicket::new();
        self.enqueue(
            event,
            current.op_id,
            current.actor,
            Some(ticket.clone()),
            false,
        )?;
        ticket.wait(self.cfg.commit_wait)
    }
}
