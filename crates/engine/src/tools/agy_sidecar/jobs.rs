use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const KEEP_DONE: Duration = Duration::from_secs(3600);
pub const KEEP_TAKEN: Duration = Duration::from_secs(600);
pub const SLICE: Duration = Duration::from_millis(250);
pub const STOP_KEEP: Duration = Duration::from_secs(120);
pub const MAX_PENDING: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Reply {
        session: String,
    },
    Start {
        model: Option<String>,
        title: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub kind: Kind,
    pub text: String,
}

impl Job {
    pub fn to_json(&self) -> Value {
        match &self.kind {
            Kind::Reply { session } => json!({
                "id": self.id, "kind": "reply", "session": session, "text": self.text,
            }),
            Kind::Start { model, title } => json!({
                "id": self.id, "kind": "start", "model": model, "title": title,
                "text": self.text,
            }),
        }
    }

    pub fn from_json(value: &Value) -> Option<Job> {
        let text = value.get("text")?.as_str()?.to_string();
        let id = value.get("id")?.as_str()?.to_string();
        let kind = match value.get("kind")?.as_str()? {
            "reply" => Kind::Reply {
                session: value.get("session")?.as_str()?.to_string(),
            },
            "start" => Kind::Start {
                model: value.get("model").and_then(Value::as_str).map(String::from),
                title: value.get("title").and_then(Value::as_str).map(String::from),
            },
            _ => return None,
        };
        Some(Job { id, kind, text })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Delivered { session: Option<String> },
    Unconfirmed { session: Option<String> },
    Failed(String),
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Delivered { session } => json!({"result": "delivered", "session": session}),
            Outcome::Unconfirmed { session } => {
                json!({"result": "unconfirmed", "session": session})
            }
            Outcome::Failed(why) => json!({"result": "failed", "error": why}),
        }
    }

    pub fn from_json(value: &Value) -> Option<Outcome> {
        let session = value
            .get("session")
            .and_then(Value::as_str)
            .map(String::from);
        match value.get("result")?.as_str()? {
            "delivered" => Some(Outcome::Delivered { session }),
            "unconfirmed" => Some(Outcome::Unconfirmed { session }),
            "failed" => Some(Outcome::Failed(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("it failed")
                    .chars()
                    .take(300)
                    .collect(),
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cancelled {
    Withdrawn,
    Taken,
    Gone,
}

#[derive(Clone, Copy, Debug)]
pub struct Patience {
    pub pickup: Duration,
    pub run: Duration,
}

#[derive(Debug)]
enum State {
    Pending,
    Taken(Instant),
    Done(Outcome, Instant),
}

#[derive(Default)]
struct Inner {
    jobs: BTreeMap<u64, (Job, State)>,
    next: u64,
    seen: Option<(i64, Option<u32>)>,
    closed: bool,
    waiters: usize,
    epoch: u64,
}

impl Inner {
    fn prune(&mut self, keep_taken: Duration) {
        self.jobs.retain(|_, (_, s)| match s {
            State::Done(_, at) => at.elapsed() <= KEEP_DONE,
            State::Taken(at) => at.elapsed() <= keep_taken,
            State::Pending => true,
        });
    }
}

pub struct Queue {
    inner: Mutex<Inner>,
    changed: Condvar,
    keep_taken: Duration,
}

impl Default for Queue {
    fn default() -> Queue {
        Queue::keeping_taken(KEEP_TAKEN)
    }
}

impl Queue {
    pub fn new() -> Queue {
        Queue::default()
    }

    pub fn keeping_taken(keep_taken: Duration) -> Queue {
        Queue {
            inner: Mutex::new(Inner::default()),
            changed: Condvar::new(),
            keep_taken,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn submit(&self, job: Job) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.closed {
            return Err("Notchling is stopping".into());
        }
        let failed_before = inner
            .jobs
            .iter()
            .find(|(_, (known, state))| {
                known.id == job.id && matches!(state, State::Done(Outcome::Failed(_), _))
            })
            .map(|(key, _)| *key);
        if let Some(key) = failed_before {
            inner.jobs.remove(&key);
        }
        if inner.jobs.values().any(|(known, _)| known.id == job.id) {
            return Ok(());
        }
        inner.prune(self.keep_taken);
        let pending = inner
            .jobs
            .values()
            .filter(|(_, s)| matches!(s, State::Pending))
            .count();
        if pending >= MAX_PENDING {
            return Err("too many messages are waiting for the Antigravity sidecar".into());
        }
        inner.next += 1;
        let n = inner.next;
        inner.jobs.insert(n, (job, State::Pending));
        self.changed.notify_all();
        Ok(())
    }

    pub fn seen(&self, now_ms: i64) {
        self.lock().seen = Some((now_ms, None));
    }

    pub fn seen_by(&self, now_ms: i64, pid: u32) {
        self.lock().seen = Some((now_ms, Some(pid)));
    }

    pub fn last_pid(&self) -> Option<u32> {
        self.lock().seen.and_then(|(_, pid)| pid)
    }

    pub fn alive(&self, now_ms: i64, within_ms: i64) -> bool {
        let mut inner = self.lock();
        inner.prune(self.keep_taken);
        let busy = inner.jobs.values().any(
            |(_, s)| matches!(s, State::Taken(at) if at.elapsed() < super::deliver::RUN_BUDGET),
        );
        inner.waiters > 0 || busy || inner.seen.is_some_and(|(at, _)| now_ms - at <= within_ms)
    }

    pub fn take(&self, wait: Duration) -> Vec<Job> {
        self.take_for(wait, usize::MAX, &|| true)
    }

    pub fn take_for(&self, wait: Duration, max: usize, alive: &dyn Fn() -> bool) -> Vec<Job> {
        self.take_noting(wait, max, alive, None)
    }

    pub fn take_noting(
        &self,
        wait: Duration,
        max: usize,
        alive: &dyn Fn() -> bool,
        note: Option<(&dyn Fn() -> i64, u32)>,
    ) -> Vec<Job> {
        let deadline = Instant::now() + wait;
        let mut inner = self.lock();
        inner.epoch += 1;
        let mine = inner.epoch;
        inner.waiters += 1;
        if let Some((clock, pid)) = note {
            if alive() {
                inner.seen = Some((clock(), Some(pid)));
            }
        }
        self.changed.notify_all();
        let got = loop {
            if inner.closed || inner.epoch != mine || !alive() {
                break Vec::new();
            }
            let mut got = Vec::new();
            for (job, state) in inner.jobs.values_mut() {
                if got.len() >= max {
                    break;
                }
                if matches!(state, State::Pending) {
                    *state = State::Taken(Instant::now());
                    got.push(job.clone());
                }
            }
            if !got.is_empty() {
                break got;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break got;
            }
            inner = self
                .changed
                .wait_timeout(inner, left.min(SLICE))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        inner.waiters -= 1;
        if let Some((clock, pid)) = note {
            if inner.epoch == mine && alive() {
                inner.seen = Some((clock(), Some(pid)));
            }
        }
        got
    }

    pub fn report(&self, id: &str, outcome: Outcome) -> bool {
        let mut inner = self.lock();
        let Some((_, state)) = inner
            .jobs
            .values_mut()
            .find(|(job, state)| job.id == id && matches!(state, State::Taken(_)))
        else {
            return false;
        };
        *state = State::Done(outcome, Instant::now());
        self.changed.notify_all();
        true
    }

    pub fn wait(&self, id: &str, timeout: Duration) -> Option<Outcome> {
        self.wait_patient(
            id,
            Patience {
                pickup: timeout,
                run: timeout,
            },
            &|| true,
        )
    }

    pub fn wait_patient(
        &self,
        id: &str,
        patience: Patience,
        still_up: &dyn Fn() -> bool,
    ) -> Option<Outcome> {
        let started = Instant::now();
        let mut inner = self.lock();
        let mut check_due = true;
        loop {
            let (deadline, check) = match inner.jobs.values().find(|(job, _)| job.id == id) {
                None => return None,
                Some((_, State::Done(outcome, _))) => return Some(outcome.clone()),
                Some(_) if inner.closed => return None,
                Some((_, State::Pending)) => (started + patience.pickup, true),
                Some((_, State::Taken(at))) => (*at + patience.run, false),
            };
            if check && check_due {
                drop(inner);
                let up = still_up();
                inner = self.lock();
                check_due = false;
                if !up {
                    return None;
                }
                continue;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            inner = self
                .changed
                .wait_timeout(inner, left.min(SLICE))
                .unwrap_or_else(|e| e.into_inner())
                .0;
            check_due = true;
        }
    }

    pub fn cancel(&self, id: &str) -> Cancelled {
        let mut inner = self.lock();
        let key = inner
            .jobs
            .iter()
            .find(|(_, (job, _))| job.id == id)
            .map(|(k, (_, s))| {
                (
                    *k,
                    matches!(s, State::Pending),
                    matches!(s, State::Taken(_)),
                )
            });
        match key {
            Some((k, true, _)) => {
                inner.jobs.remove(&k);
                self.changed.notify_all();
                Cancelled::Withdrawn
            }
            Some((_, _, true)) => Cancelled::Taken,
            _ => Cancelled::Gone,
        }
    }

    pub fn waiting(&self) -> usize {
        self.lock()
            .jobs
            .values()
            .filter(|(_, s)| matches!(s, State::Pending))
            .count()
    }

    pub fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }
}

#[derive(Default)]
pub struct Stops {
    asked: Mutex<HashMap<String, Instant>>,
}

impl Stops {
    pub fn new() -> Stops {
        Stops::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Instant>> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn ask(&self, conversation: &str) {
        let mut asked = self.lock();
        asked.retain(|_, at| at.elapsed() < STOP_KEEP);
        asked.insert(conversation.to_string(), Instant::now());
    }

    pub fn take(&self, conversation: &str) -> bool {
        let mut asked = self.lock();
        asked.retain(|_, at| at.elapsed() < STOP_KEEP);
        asked.remove(conversation).is_some()
    }

    pub fn forget(&self, conversation: &str) {
        self.lock().remove(conversation);
    }

    pub fn clear(&self) {
        self.lock().clear();
    }
}
