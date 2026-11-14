use notchling_events::emit::{Committed, Unwritable, UnwritableWhy};
use notchling_events::{Emit, Event};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Default)]
pub struct LateEmit {
    target: OnceLock<Arc<dyn Emit>>,
    lost: AtomicU64,
}

impl LateEmit {
    pub fn new() -> Arc<LateEmit> {
        Arc::new(LateEmit::default())
    }

    pub fn set(&self, target: Arc<dyn Emit>) {
        let _ = self.target.set(target);
    }

    pub fn lost(&self) -> u64 {
        self.lost.load(Ordering::SeqCst)
    }
}

impl Emit for LateEmit {
    fn push(&self, event: Event) {
        match self.target.get() {
            Some(target) => target.push(event),
            None => {
                self.lost.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    fn commit(&self, event: Event) -> Result<Committed, Unwritable> {
        match self.target.get() {
            Some(target) => target.commit(event),
            None => {
                self.lost.fetch_add(1, Ordering::SeqCst);
                Err(Unwritable {
                    why: UnwritableWhy::Closed,
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "late_emit_tests.rs"]
mod tests;
