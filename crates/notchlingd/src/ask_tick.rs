use crate::ask_ops::AskOps;
use notchling_engine::ask::{AskState, Mode, Row};
use notchling_events::kinds::choices::AskThen;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) const TICK: Duration = Duration::from_secs(1);
pub(crate) const SWEEP_EVERY_MS: i64 = 3_600_000;
pub(crate) const NEVER: i64 = i64::MIN;

pub(crate) enum Msg {
    Stop,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AskOps {
    pub(crate) fn asker_alive(&self, row: &Row) -> bool {
        let now = self.procs.start_time(row.pid);
        match row.pid_start {
            Some(start) => now == Some(start),
            None => now.is_some() || self.procs.parent(row.pid).is_some(),
        }
    }

    fn end_row(&self, id: &str, from: &[AskState], to: AskState, then: AskThen, now_ms: i64) {
        if !matches!(self.store.close(id, from, to, now_ms), Ok(true)) {
            return;
        }
        if let Ok(Some(row)) = self.store.get(id) {
            self.moved(&row, then);
        }
    }

    pub fn tick(&self, now_ms: i64) {
        if let Ok(rows) = self.store.live() {
            for row in rows {
                let late = row.mode == Mode::Wait
                    && row.state == AskState::Open
                    && row.deadline_ms.is_some_and(|at| now_ms >= at);
                if late {
                    self.end_row(
                        &row.id,
                        &[AskState::Open],
                        AskState::Expired,
                        AskThen::Expired,
                        now_ms,
                    );
                } else if !self.waiters.connected(&row.id) && !self.asker_alive(&row) {
                    self.end_row(
                        &row.id,
                        &[AskState::Open, AskState::Answered],
                        AskState::Gone,
                        AskThen::Gone,
                        now_ms,
                    );
                }
            }
        }
        let last = self.last_sweep.load(Ordering::SeqCst);
        if last == NEVER || now_ms.saturating_sub(last) >= SWEEP_EVERY_MS {
            self.last_sweep.store(now_ms, Ordering::SeqCst);
            self.sweep(now_ms);
        }
    }

    pub fn sweep(&self, now_ms: i64) {
        let Ok(rows) = self.store.sweep(now_ms) else {
            return;
        };
        for row in rows {
            self.waiters.release(&row.id);
            self.room.publish(
                notchling_local_api::topics::ASKS,
                notchling_local_api::topics::ASK_REMOVED,
                &json!({"id": row.id, "session": row.session}),
            );
        }
    }

    pub fn load(&self, now_ms: i64) {
        let Ok(rows) = self.store.live() else {
            return;
        };
        for row in &rows {
            self.emit_at(row, AskThen::Restored, now_ms);
        }
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        self.spawn_loop_every(TICK);
    }

    pub fn spawn_loop_every(self: &Arc<Self>, every: Duration) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let Some(rx) = lock(&self.rx).take() else {
            return;
        };
        let weak = Arc::downgrade(self);
        *thread = std::thread::Builder::new()
            .name("notchling-ask".into())
            .spawn(move || {
                let mut last = Instant::now();
                loop {
                    let wait = every.saturating_sub(last.elapsed());
                    let msg = rx.recv_timeout(wait);
                    let Some(ops) = weak.upgrade() else {
                        return;
                    };
                    match msg {
                        Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    if last.elapsed() >= every {
                        last = Instant::now();
                        ops.tick(ops.now());
                    }
                }
            })
            .ok();
    }

    pub fn close(&self) {
        self.waiters.close();
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = self.tx.send(Msg::Stop);
            let _ = handle.join();
        }
    }
}

pub(crate) fn queue() -> (
    std::sync::mpsc::Sender<Msg>,
    Mutex<Option<std::sync::mpsc::Receiver<Msg>>>,
) {
    let (tx, rx) = channel();
    (tx, Mutex::new(Some(rx)))
}

#[cfg(test)]
#[path = "ask_tick_tests.rs"]
mod tests;
