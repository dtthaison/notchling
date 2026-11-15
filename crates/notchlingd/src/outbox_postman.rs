use crate::acting::Acting;
use crate::outbox_courier::{real_clock, Clock, Courier, Refuse};
use crate::outbox_ops::{OutboxOps, Wake};
use notchling_engine::outbox::{
    after, gate, After, Gate, Job, QueuedVerdict, Row, State, Tool, TryEnd, Verdict, Why,
    ATTACH_IDLE_MS, GONE_RECHECK_MS, POSTMEN_MAX,
};
use notchling_engine::transcript::sent::Seen;
use notchling_events::kinds::agents::OutboxThen;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

pub const TICK_MS: i64 = 1_000;
const LAND_TRIES: u32 = 3;
const LAND_BACKOFF_MS: u64 = 40;

pub trait Verifier: Send + Sync {
    fn seen(&self, row: &Row) -> Option<Seen>;
    fn wait(&self, row: &Row, before: Seen, entered_ms: i64) -> Verdict;
    fn watch(&self, row: &Row, before: Seen, now_ms: i64) -> QueuedVerdict;
    fn next_look_ms(&self, row: &Row) -> Option<i64>;
    fn forget(&self, _id: &str) {}
    fn close(&self) {}
}

pub struct NoVerify;

impl Verifier for NoVerify {
    fn seen(&self, _row: &Row) -> Option<Seen> {
        None
    }

    fn wait(&self, _row: &Row, _before: Seen, _entered_ms: i64) -> Verdict {
        Verdict::Sent
    }

    fn watch(&self, _row: &Row, _before: Seen, _now_ms: i64) -> QueuedVerdict {
        QueuedVerdict::Sent
    }

    fn next_look_ms(&self, _row: &Row) -> Option<i64> {
        None
    }
}

pub struct Couriers {
    pub claude: Arc<dyn Courier>,
    pub codex: Arc<dyn Courier>,
    pub antigravity: Arc<dyn Courier>,
}

impl Couriers {
    pub fn claude_only(claude: Arc<dyn Courier>) -> Couriers {
        Couriers {
            claude,
            codex: Arc::new(Refuse),
            antigravity: Arc::new(Refuse),
        }
    }

    fn of(&self, tool: Tool) -> &Arc<dyn Courier> {
        match tool {
            Tool::Claude => &self.claude,
            Tool::Codex => &self.codex,
            Tool::Antigravity => &self.antigravity,
        }
    }
}

enum Msg {
    Wake,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    At(i64),
    Done,
}

struct Running {
    tx: Sender<Msg>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Registry {
    running: HashMap<String, Running>,
    pending: Vec<String>,
    begun: bool,
    stopped: bool,
}

struct Inner {
    ops: Arc<OutboxOps>,
    couriers: Couriers,
    verifier: Arc<dyn Verifier>,
    acting: Arc<Acting>,
    clock: Clock,
    registry: Mutex<Registry>,
    stopped: AtomicBool,
}

impl Inner {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct Postmen {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Postmen {
    pub fn start(
        ops: Arc<OutboxOps>,
        couriers: Couriers,
        verifier: Arc<dyn Verifier>,
        acting: Arc<Acting>,
    ) -> Postmen {
        let postmen = Postmen::build(ops, couriers, verifier, acting, real_clock());
        postmen.begin();
        postmen
    }

    pub fn build(
        ops: Arc<OutboxOps>,
        couriers: Couriers,
        verifier: Arc<dyn Verifier>,
        acting: Arc<Acting>,
        clock: Clock,
    ) -> Postmen {
        let forgetting = verifier.clone();
        ops.set_forget(Arc::new(move |id| forgetting.forget(id)));
        Postmen {
            inner: Arc::new(Inner {
                ops,
                couriers,
                verifier,
                acting,
                clock,
                registry: Mutex::new(Registry::default()),
                stopped: AtomicBool::new(false),
            }),
        }
    }

    pub fn begin(&self) {
        {
            let mut registry = lock(&self.inner.registry);
            if registry.begun || registry.stopped {
                return;
            }
            registry.begun = true;
        }
        let weak = Arc::downgrade(&self.inner);
        self.inner.acting.watch(Arc::new(move |on| {
            if let Some(inner) = weak.upgrade() {
                match on {
                    true => Postmen { inner }.wake_all(),
                    false => Postmen { inner }.wake_running(),
                }
            }
        }));
        self.wake_all();
    }

    pub fn waker(&self) -> Wake {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        Arc::new(move |session| {
            if let Some(inner) = weak.upgrade() {
                Postmen { inner }.wake(session);
            }
        })
    }

    pub fn wake(&self, session: &str) {
        let mut registry = lock(&self.inner.registry);
        if !registry.begun || registry.stopped {
            return;
        }
        if let Some(running) = registry.running.get(session) {
            let _ = running.tx.send(Msg::Wake);
            return;
        }
        if registry.running.len() < POSTMEN_MAX {
            self.spawn(&mut registry, session);
        } else if !registry.pending.iter().any(|s| s == session) {
            registry.pending.push(session.to_string());
        }
    }

    fn wake_running(&self) {
        let registry = lock(&self.inner.registry);
        for running in registry.running.values() {
            let _ = running.tx.send(Msg::Wake);
        }
    }

    pub fn wake_all(&self) {
        let sessions = self.inner.ops.store().sessions().unwrap_or_default();
        for session in sessions {
            self.wake(&session);
        }
    }

    pub fn stop(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        self.inner.verifier.close();
        let all: Vec<Running> = {
            let mut registry = lock(&self.inner.registry);
            registry.stopped = true;
            registry.pending.clear();
            registry.running.drain().map(|(_, r)| r).collect()
        };
        for running in &all {
            let _ = running.tx.send(Msg::Stop);
        }
        for running in all {
            if let Some(handle) = running.handle {
                let _ = handle.join();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn running(&self) -> usize {
        lock(&self.inner.registry).running.len()
    }

    #[cfg(test)]
    pub(crate) fn postman(&self, session: &str) -> Postman {
        Postman::new(self.inner.clone(), session)
    }

    fn spawn(&self, registry: &mut Registry, session: &str) {
        let (tx, rx) = channel();
        let inner = self.inner.clone();
        let name = session.to_string();
        let spawned = std::thread::Builder::new()
            .name("notchling-postman".into())
            .spawn(move || run(inner, name, rx));
        if let Ok(handle) = spawned {
            registry.running.insert(
                session.to_string(),
                Running {
                    tx,
                    handle: Some(handle),
                },
            );
        }
    }
}

fn run(inner: Arc<Inner>, session: String, rx: Receiver<Msg>) {
    let mut postman = Postman::new(inner.clone(), &session);
    loop {
        match postman.pass((inner.clock)()) {
            Next::At(at) => {
                let wait = (at - (inner.clock)()).clamp(0, TICK_MS);
                match rx.recv_timeout(Duration::from_millis(wait as u64)) {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Msg::Wake) | Err(RecvTimeoutError::Timeout) => {}
                }
                if inner.stopped() {
                    break;
                }
            }
            Next::Done => {
                postman.close_attach();
                if finish(&inner, &session, &rx) {
                    return;
                }
            }
        }
    }
    postman.close_attach();
}

fn finish(inner: &Arc<Inner>, session: &str, rx: &Receiver<Msg>) -> bool {
    if inner.stopped() {
        return true;
    }
    let mut registry = lock(&inner.registry);
    match rx.try_recv() {
        Ok(Msg::Wake) => return false,
        Ok(Msg::Stop) | Err(TryRecvError::Disconnected) | Err(TryRecvError::Empty) => {}
    }
    if registry.stopped {
        return true;
    }
    registry.running.remove(session);
    let postmen = Postmen {
        inner: inner.clone(),
    };
    while registry.running.len() < POSTMEN_MAX && !registry.pending.is_empty() {
        let next = registry.pending.remove(0);
        if !registry.running.contains_key(&next) {
            postmen.spawn(&mut registry, &next);
        }
    }
    true
}

enum Owed {
    Sent(Row),
    Moved(Move),
}

struct Move {
    row: Row,
    from: State,
    to: State,
    why: Option<Why>,
    next_ms: Option<i64>,
    then: Option<OutboxThen>,
}

pub(crate) struct Postman {
    inner: Arc<Inner>,
    session: String,
    attach_until: Option<i64>,
    gone_checked: Option<i64>,
    courier: Option<Arc<dyn Courier>>,
    owed: Option<Owed>,
}

fn baseline(row: &Row) -> Seen {
    let count = |n: Option<i64>| n.and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    Seen {
        taken: count(row.seen_before),
        enqueued: count(row.queued_before),
    }
}

impl Postman {
    fn new(inner: Arc<Inner>, session: &str) -> Postman {
        Postman {
            inner,
            session: session.to_string(),
            attach_until: None,
            gone_checked: None,
            courier: None,
            owed: None,
        }
    }

    pub(crate) fn pass(&mut self, now: i64) -> Next {
        if !self.inner.acting.is_on() || self.inner.stopped() {
            self.close_attach();
            return Next::Done;
        }
        loop {
            if !self.inner.acting.is_on() || self.inner.stopped() {
                self.close_attach();
                return Next::Done;
            }
            if let Some(owed) = self.owed.take() {
                if !self.land(owed) {
                    return self.rest(now, Some(now + TICK_MS));
                }
            }
            let row = match self.inner.ops.store().head(&self.session) {
                Ok(Some(row)) => row,
                Ok(None) => return self.rest(now, None),
                Err(_) => return self.rest(now, Some(now + TICK_MS)),
            };
            self.courier = Some(self.inner.couriers.of(row.tool).clone());
            let timer = match row.state {
                State::Unsure | State::Box | State::Stopped | State::Sending => {
                    return self.rest(now, None)
                }
                State::Gone => self.recheck_gone(&row, now),
                State::InQueue => self.watch(&row, now),
                State::Queued | State::Waiting => self.gate(&row, now),
            };
            if let Some(at) = timer {
                return self.rest(now, Some(at));
            }
        }
    }

    pub(crate) fn close_attach(&mut self) {
        if self.attach_until.take().is_some() {
            if let Some(courier) = &self.courier {
                courier.close(&self.session);
            }
        }
    }

    fn rest(&mut self, now: i64, timer: Option<i64>) -> Next {
        let mut next = timer;
        if let Some(until) = self.attach_until {
            if now >= until {
                self.close_attach();
            } else {
                next = Some(next.map_or(until, |t| t.min(until)));
            }
        }
        match next {
            Some(at) => Next::At(at.max(now + 1)),
            None => Next::Done,
        }
    }

    fn courier(&self) -> Arc<dyn Courier> {
        self.courier
            .clone()
            .unwrap_or_else(|| self.inner.couriers.claude.clone())
    }

    fn recheck_gone(&mut self, row: &Row, now: i64) -> Option<i64> {
        if let Some(at) = self.gone_checked.filter(|at| now - at < GONE_RECHECK_MS) {
            return Some(at + GONE_RECHECK_MS);
        }
        self.gone_checked = Some(now);
        let facts = self.courier().facts(row, now);
        if matches!(facts.job, Job::Missing { .. }) {
            return Some(now + GONE_RECHECK_MS);
        }
        let store = self.inner.ops.store();
        if store
            .requeue(&row.id, &[State::Gone], row.clear_box, now)
            .is_err()
        {
            return Some(now + TICK_MS);
        }
        self.gone_checked = None;
        self.publish(&row.id);
        None
    }

    fn watch(&mut self, row: &Row, now: i64) -> Option<i64> {
        let verifier = &self.inner.verifier;
        match verifier.watch(row, baseline(row), now) {
            QueuedVerdict::Sent => {
                self.sent(row);
                None
            }
            QueuedVerdict::Unsure => {
                self.moved(
                    row,
                    State::InQueue,
                    State::Unsure,
                    Some(Why::Dropped),
                    None,
                    Some(OutboxThen::Unsure),
                );
                None
            }
            QueuedVerdict::Stay => {
                let tick = now + TICK_MS;
                Some(verifier.next_look_ms(row).map_or(tick, |at| at.min(tick)))
            }
        }
    }

    fn gate(&mut self, row: &Row, now: i64) -> Option<i64> {
        let facts = self.courier().facts(row, now);
        match gate(row, &facts) {
            Gate::Wait { why, until_ms } => {
                if row.state != State::Waiting || row.why != Some(why) {
                    let store = self.inner.ops.store();
                    let from = [State::Queued, State::Waiting];
                    if let Ok(true) =
                        store.set_state(&row.id, &from, State::Waiting, Some(why), row.next_ms, now)
                    {
                        self.publish(&row.id);
                    }
                }
                Some(until_ms)
            }
            Gate::NeedsUser(state) => {
                let then = match state {
                    State::Stopped => Some(OutboxThen::Stopped),
                    State::Gone => Some(OutboxThen::Gone),
                    _ => None,
                };
                if state == State::Gone {
                    self.gone_checked = Some(now);
                }
                let store = self.inner.ops.store();
                let from = [State::Queued, State::Waiting];
                if let Ok(true) = store.set_state(&row.id, &from, state, None, None, now) {
                    self.publish(&row.id);
                    if let Some(then) = then {
                        self.inner.ops.emit_then(row, then);
                    }
                }
                None
            }
            Gate::Go { guarded } => self.try_row(row, guarded, now),
        }
    }

    fn try_row(&mut self, row: &Row, guarded: bool, now: i64) -> Option<i64> {
        let store = self.inner.ops.store();
        let taken = match store.take(&row.id, now) {
            Ok(Some(taken)) => taken,
            Ok(None) => return None,
            Err(_) => return Some(now + TICK_MS),
        };
        if taken.kind != row.kind || taken.text != row.text {
            let again = store.requeue(&taken.id, &[State::Sending], taken.clear_box, now);
            return match again {
                Ok(_) => {
                    self.publish(&taken.id);
                    None
                }
                Err(_) => Some(now + TICK_MS),
            };
        }
        self.inner.ops.publish_changed(&taken);
        let courier = self.courier();
        let verifies = courier.verifies();
        let before = if verifies {
            let seen = self.inner.verifier.seen(&taken).unwrap_or_default();
            let _ = store.set_seen(&taken.id, seen);
            seen
        } else {
            Seen::default()
        };
        let (end, tried, note) = courier.deliver(&taken, guarded);
        let ended = (self.inner.clock)();
        self.attach_until = (end == TryEnd::Ok).then_some(ended + ATTACH_IDLE_MS);
        let rested = match after(&end, tried, taken.tries, ended) {
            After::Verify if !verifies => self.sent(&taken),
            After::Verify => match self.inner.verifier.wait(&taken, before, ended) {
                Verdict::Sent => self.sent(&taken),
                Verdict::InQueue => {
                    let then = Some(OutboxThen::InQueue);
                    self.moved(&taken, State::Sending, State::InQueue, None, None, then);
                    false
                }
                Verdict::Pending => true,
                Verdict::Unsure => {
                    let then = Some(OutboxThen::Unsure);
                    let why = Some(Why::NotFound);
                    self.moved(&taken, State::Sending, State::Unsure, why, None, then)
                }
            },
            After::Unsure(why) => {
                let then = Some(OutboxThen::Unsure);
                self.moved(&taken, State::Sending, State::Unsure, Some(why), None, then)
            }
            After::Box(why) => {
                self.inner.ops.set_note(&taken.id, note);
                let then = Some(OutboxThen::Box);
                self.moved(&taken, State::Sending, State::Box, Some(why), None, then)
            }
            After::Wait { why, next_ms } => {
                if why == Why::Retry {
                    self.inner.ops.set_note(&taken.id, note);
                }
                self.moved(
                    &taken,
                    State::Sending,
                    State::Waiting,
                    Some(why),
                    next_ms,
                    None,
                );
                false
            }
        };
        if rested {
            None
        } else {
            Some(ended + TICK_MS)
        }
    }

    fn sent(&mut self, row: &Row) -> bool {
        self.settle(Owed::Sent(row.clone()))
    }

    fn moved(
        &mut self,
        row: &Row,
        from: State,
        to: State,
        why: Option<Why>,
        next_ms: Option<i64>,
        then: Option<OutboxThen>,
    ) -> bool {
        self.settle(Owed::Moved(Move {
            row: row.clone(),
            from,
            to,
            why,
            next_ms,
            then,
        }))
    }

    fn settle(&mut self, owed: Owed) -> bool {
        for attempt in 0..LAND_TRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(LAND_BACKOFF_MS * u64::from(attempt)));
            }
            if self.try_land(&owed) {
                return true;
            }
        }
        self.owed = Some(owed);
        false
    }

    fn land(&mut self, owed: Owed) -> bool {
        if self.try_land(&owed) {
            return true;
        }
        self.owed = Some(owed);
        false
    }

    fn try_land(&self, owed: &Owed) -> bool {
        match owed {
            Owed::Sent(row) => {
                if self.inner.ops.store().delete(&row.id).is_err() {
                    return false;
                }
                self.inner
                    .ops
                    .publish_removed(&row.id, &row.session, "sent");
                self.inner.ops.emit_then(row, OutboxThen::Sent);
                true
            }
            Owed::Moved(step) => {
                let now = (self.inner.clock)();
                let store = self.inner.ops.store();
                let landed = store.set_state(
                    &step.row.id,
                    &[step.from],
                    step.to,
                    step.why,
                    step.next_ms,
                    now,
                );
                let Ok(changed) = landed else {
                    return false;
                };
                if changed {
                    self.publish(&step.row.id);
                    if let Some(then) = step.then {
                        self.inner.ops.emit_then(&step.row, then);
                    }
                }
                true
            }
        }
    }

    fn publish(&self, id: &str) {
        if let Ok(Some(row)) = self.inner.ops.store().get(id) {
            self.inner.ops.publish_changed(&row);
        }
    }
}

#[cfg(test)]
#[path = "outbox_postman_tests.rs"]
mod tests;
