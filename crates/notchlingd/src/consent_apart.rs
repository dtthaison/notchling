use crate::consent_ops::{ConsentOps, Failure, Msg};
use notchling_engine::consent::{Consent, State};
use notchling_events::kinds::choices::ConsentThen;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const JOIN_MS: u64 = 5_000;
const POLL: Duration = Duration::from_millis(10);
const STOPPING: &str = "stopping";

pub(crate) struct Applies {
    handles: Mutex<Vec<JoinHandle<()>>>,
    closing: AtomicBool,
    done: AtomicBool,
    join_ms: AtomicU64,
}

impl Default for Applies {
    fn default() -> Applies {
        Applies {
            handles: Mutex::new(Vec::new()),
            closing: AtomicBool::new(false),
            done: AtomicBool::new(false),
            join_ms: AtomicU64::new(JOIN_MS),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Applies {
    pub(crate) fn closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn join_within(&self, ms: u64) {
        self.join_ms.store(ms, Ordering::SeqCst);
    }

    pub(crate) fn stop_new(&self) {
        let _handles = lock(&self.handles);
        self.closing.store(true, Ordering::SeqCst);
    }

    pub(crate) fn spawn(&self, work: impl FnOnce() + Send + 'static) -> bool {
        self.start(false, work)
    }

    pub(crate) fn spawn_late(&self, work: impl FnOnce() + Send + 'static) -> bool {
        self.start(true, work)
    }

    fn start(&self, late: bool, work: impl FnOnce() + Send + 'static) -> bool {
        let mut handles = lock(&self.handles);
        let refused = if late {
            self.done.load(Ordering::SeqCst)
        } else {
            self.closing()
        };
        if refused {
            return false;
        }
        let Ok(handle) = std::thread::Builder::new()
            .name("notchling-consent-apply".into())
            .spawn(work)
        else {
            return false;
        };
        handles.retain(|h| !h.is_finished());
        handles.push(handle);
        true
    }

    pub(crate) fn close(&self) {
        let until = Instant::now() + Duration::from_millis(self.join_ms.load(Ordering::SeqCst));
        loop {
            let batch = {
                let mut handles = lock(&self.handles);
                self.closing.store(true, Ordering::SeqCst);
                let batch = std::mem::take(&mut *handles);
                if batch.is_empty() {
                    self.done.store(true, Ordering::SeqCst);
                }
                batch
            };
            if batch.is_empty() {
                return;
            }
            for handle in batch {
                while !handle.is_finished() && Instant::now() < until {
                    std::thread::sleep(POLL);
                }
            }
            if Instant::now() >= until {
                let _handles = lock(&self.handles);
                self.done.store(true, Ordering::SeqCst);
                return;
            }
        }
    }
}

impl ConsentOps {
    pub(crate) fn apply_apart(&self, row: Consent) {
        let Some(me) = self.me.upgrade() else {
            return;
        };
        let again = row.clone();
        if self.applies.spawn(move || me.apply(row)) {
            return;
        }
        if self.applies.closing() {
            self.end_failed(&again, &stopping());
        } else {
            self.apply(again);
        }
    }

    pub(crate) fn deliver(&self, msg: Msg) {
        let back = match lock(&self.queue).as_ref() {
            Some(queue) => queue.send(msg).err().map(|failed| failed.0),
            None if self.applies.closing() => Some(msg),
            None => None,
        };
        let Some(back) = back else {
            return;
        };
        let Some(me) = self.me.upgrade() else {
            return;
        };
        let again = back.clone();
        if !self.applies.spawn_late(move || me.settle_late(back)) {
            self.stop_in_store(&again);
        }
    }

    pub(crate) fn settle_late(&self, msg: Msg) {
        match msg {
            Msg::Settle(id) => match self.store.get(&id) {
                Ok(Some(row)) if row.state == State::Denied => {
                    self.closed(&row, ConsentThen::Denied)
                }
                Ok(Some(row)) if row.state == State::Applying => {
                    let stored = stopping().stored();
                    let won = self
                        .store
                        .finish(&id, State::Failed, stored.as_ref(), self.now())
                        .ok()
                        .flatten();
                    if let Some(done) = won {
                        self.closed(&done, ConsentThen::Failed);
                    }
                }
                _ => {}
            },
            Msg::Withdrawn(id) => self.withdrawn_at_allow(&id),
            Msg::Stop => {}
        }
    }

    fn stop_in_store(&self, msg: &Msg) {
        let Msg::Settle(id) = msg else {
            return;
        };
        let Ok(Some(row)) = self.store.get(id) else {
            return;
        };
        if row.state == State::Applying {
            let stored = stopping().stored();
            let _ = self
                .store
                .finish(id, State::Failed, stored.as_ref(), self.now());
        }
    }
}

fn stopping() -> Failure {
    Failure::from(STOPPING.to_string())
}
