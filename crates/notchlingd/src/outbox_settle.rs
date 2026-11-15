use crate::outbox_ops::OutboxOps;
use notchling_engine::outbox::{Row, State};
use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

const LOOK_AGAIN: Duration = Duration::from_millis(250);
const REMEMBERED: usize = 512;
const UNKNOWN_GRACE_MS: i64 = 2_000;

#[derive(Debug)]
pub enum Settled {
    Sent,
    Removed,
    InQueue(Row),
    Pending(Row),
    NeedsUser(Row, Option<String>),
}

#[derive(Default)]
pub struct Settle {
    removals: Mutex<Removals>,
    changes: Mutex<u64>,
    changed: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Removals {
    why: HashMap<String, String>,
    order: VecDeque<String>,
}

impl Settle {
    pub fn removed(&self, id: &str, why: &str) {
        let mut removals = lock(&self.removals);
        if removals
            .why
            .insert(id.to_string(), why.to_string())
            .is_none()
        {
            removals.order.push_back(id.to_string());
        }
        while removals.order.len() > REMEMBERED {
            if let Some(old) = removals.order.pop_front() {
                removals.why.remove(&old);
            }
        }
    }

    fn removal(&self, id: &str) -> Option<String> {
        lock(&self.removals).why.get(id).cloned()
    }

    pub fn bump(&self) {
        *lock(&self.changes) += 1;
        self.changed.notify_all();
    }

    fn seen(&self) -> u64 {
        *lock(&self.changes)
    }

    fn wait_past(&self, seen: u64) {
        let changes = lock(&self.changes);
        if *changes == seen {
            let _ = self.changed.wait_timeout(changes, LOOK_AGAIN);
        }
    }
}

fn needs_user(state: State) -> bool {
    matches!(
        state,
        State::Unsure | State::Box | State::Stopped | State::Gone
    )
}

impl OutboxOps {
    pub fn wait_settled(&self, sent: &Row, max_ms: i64) -> Settled {
        let deadline = self.now() + max_ms;
        let mut last = sent.clone();
        let mut missing_since: Option<i64> = None;
        loop {
            let seen = self.settle().seen();
            match self.store().get(&sent.id) {
                Ok(None) => match self.settle().removal(&sent.id).as_deref() {
                    Some("sent") => return Settled::Sent,
                    Some(_) => return Settled::Removed,
                    None => {
                        let since = *missing_since.get_or_insert(self.now());
                        if self.now() - since >= UNKNOWN_GRACE_MS {
                            return Settled::Removed;
                        }
                    }
                },
                Ok(Some(row)) if row.state == State::InQueue => return Settled::InQueue(row),
                Ok(Some(row)) if needs_user(row.state) => {
                    let note = self.note(&row.id);
                    return Settled::NeedsUser(row, note);
                }
                Ok(Some(row)) => last = row,
                Err(_) => {}
            }
            if self.now() >= deadline {
                return Settled::Pending(last);
            }
            self.settle().wait_past(seen);
        }
    }
}
