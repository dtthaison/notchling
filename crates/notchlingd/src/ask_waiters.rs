use crate::acting_ops::forbidden;
use crate::ask_ops::AskOps;
use notchling_engine::ask::{AskState, Row, WAIT_MS_MAX};
use notchling_events::kinds::choices::AskThen;
use notchling_local_api::{ApiError, Caller};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const NOT_YOURS: &str = "Only the command that asked can wait for this answer.";

#[derive(Default)]
struct Slot {
    state: Mutex<Counts>,
    wake: Condvar,
}

#[derive(Default)]
struct Counts {
    woken: u64,
    connected: usize,
}

#[derive(Default)]
pub(crate) struct Waiters {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    closed: AtomicBool,
}

pub(crate) struct Present {
    slot: Arc<Slot>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Waiters {
    fn slot(&self, id: &str) -> Arc<Slot> {
        lock(&self.slots).entry(id.to_string()).or_default().clone()
    }

    pub(crate) fn enter(&self, id: &str) -> Present {
        let slot = self.slot(id);
        lock(&slot.state).connected += 1;
        Present { slot }
    }

    pub(crate) fn connected(&self, id: &str) -> bool {
        let slot = lock(&self.slots).get(id).cloned();
        slot.is_some_and(|slot| lock(&slot.state).connected > 0)
    }

    pub(crate) fn wake(&self, id: &str) {
        let slot = lock(&self.slots).get(id).cloned();
        if let Some(slot) = slot {
            lock(&slot.state).woken += 1;
            slot.wake.notify_all();
        }
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let all: Vec<Arc<Slot>> = lock(&self.slots).values().cloned().collect();
        for slot in all {
            lock(&slot.state).woken += 1;
            slot.wake.notify_all();
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn release(&self, id: &str) {
        let mut slots = lock(&self.slots);
        let idle = slots
            .get(id)
            .is_some_and(|slot| lock(&slot.state).connected == 0);
        if idle {
            slots.remove(id);
        }
    }
}

impl Present {
    pub(crate) fn mark(&self) -> u64 {
        lock(&self.slot.state).woken
    }

    pub(crate) fn block(&self, seen: u64, until: Instant) -> bool {
        let mut counts = lock(&self.slot.state);
        while counts.woken == seen {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            counts = self
                .slot
                .wake
                .wait_timeout(counts, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

impl Drop for Present {
    fn drop(&mut self) {
        let mut counts = lock(&self.slot.state);
        counts.connected = counts.connected.saturating_sub(1);
    }
}

pub(crate) fn budget(args: &Value) -> Duration {
    let ms = args
        .get("waitMs")
        .and_then(Value::as_i64)
        .unwrap_or(WAIT_MS_MAX)
        .clamp(0, WAIT_MS_MAX);
    Duration::from_millis(ms as u64)
}

fn handed(row: &Row) -> Value {
    let mut out = json!({"state": "answered", "answer": row.answer});
    if let Some(choice) = row.choice {
        out["choice"] = json!(choice);
    }
    if let Some(by) = row.by {
        out["by"] = json!(by.as_str());
    }
    out
}

impl AskOps {
    fn is_asker(&self, caller: &Caller, row: &Row) -> bool {
        let Some(pid) = caller.pid.filter(|pid| *pid > 0) else {
            return false;
        };
        pid == row.pid && self.procs.start_time(pid) == row.pid_start
    }

    fn reply_for(&self, row: &Row) -> Option<Value> {
        match row.state {
            AskState::Open => None,
            AskState::Answered => {
                let delivered = self.store.close(
                    &row.id,
                    &[AskState::Answered],
                    AskState::Delivered,
                    self.now(),
                );
                if matches!(delivered, Ok(true)) {
                    if let Ok(Some(changed)) = self.store.get(&row.id) {
                        self.moved(&changed, AskThen::Delivered);
                    }
                }
                Some(handed(row))
            }
            AskState::Delivered => Some(handed(row)),
            other => Some(json!({"state": other.as_str()})),
        }
    }

    pub(crate) fn wait_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = crate::ask_ops::id_arg(args)?;
        let first = self.row(id)?;
        if !self.is_asker(caller, &first) {
            return Err(forbidden(NOT_YOURS));
        }
        let until = Instant::now() + budget(args);
        let here = self.waiters.enter(id);
        let reply = loop {
            let seen = here.mark();
            let row = self.row(id)?;
            if let Some(reply) = self.reply_for(&row) {
                break reply;
            }
            if self.waiters.is_closed() || !here.block(seen, until) {
                break json!({"state": "open"});
            }
        };
        drop(here);
        self.waiters.release(id);
        Ok(reply)
    }
}

#[cfg(test)]
#[path = "ask_waiters_tests.rs"]
mod tests;
