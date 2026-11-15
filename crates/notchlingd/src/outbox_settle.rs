use crate::outbox_ops::OutboxOps;
use notchling_engine::outbox::{Row, State};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

const LOOK_AGAIN: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum Settled {
    Sent,
    InQueue(Row),
    Pending(Row),
    NeedsUser(Row, Option<String>),
}

#[derive(Default)]
pub struct Settle {
    changes: Mutex<u64>,
    changed: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Settle {
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
        loop {
            let seen = self.settle().seen();
            match self.store().get(&sent.id) {
                Ok(None) => return Settled::Sent,
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
