use notchling_events::emit::{Committed, Unwritable, UnwritableWhy};
use notchling_events::{Emit, Event, Salt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub type LateSalt = Arc<OnceLock<Salt>>;

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

#[derive(Clone)]
pub struct Events {
    emit: Arc<dyn Emit>,
    salt: LateSalt,
}

impl Events {
    pub fn new(emit: Arc<dyn Emit>, salt: LateSalt) -> Events {
        Events { emit, salt }
    }

    pub fn fixed(emit: Arc<dyn Emit>, salt: Salt) -> Events {
        let cell = OnceLock::new();
        let _ = cell.set(salt);
        Events::new(emit, Arc::new(cell))
    }

    pub fn salt(&self) -> Salt {
        self.salt.get().copied().unwrap_or(Salt([0; 32]))
    }

    pub fn push(&self, event: Event) {
        self.emit.push(event);
    }

    pub fn commit(&self, event: Event) -> bool {
        self.emit.commit(event).is_ok()
    }
}

#[cfg(test)]
#[path = "late_emit_tests.rs"]
mod tests;
