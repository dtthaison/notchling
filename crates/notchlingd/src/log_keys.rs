use notchling_engine::logs::{Checkpoint, KeyError, KeyRead, KeyStore};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub struct Loaded {
    pub read: KeyRead,
    pub created: Option<Result<[u8; 32], KeyError>>,
    pub checkpoint: Option<Checkpoint>,
}

#[derive(Clone, Copy)]
pub struct LoadPlan {
    pub quiet_dir: bool,
    pub moved_legacy: bool,
}

pub fn load(keys: &dyn KeyStore, plan: LoadPlan) -> Loaded {
    let checkpoint = keys.read_head();
    let fresh = plan.quiet_dir && checkpoint.is_none() && !keys.head_exists();
    let read = keys.read_key();
    let created = match read {
        KeyRead::Missing if fresh || plan.moved_legacy => Some(keys.create_key()),
        _ => None,
    };
    Loaded {
        read,
        created,
        checkpoint,
    }
}

pub fn spawn_load(keys: Arc<dyn KeyStore>, plan: LoadPlan) -> Option<Receiver<Loaded>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("notchling-log-key".into())
        .spawn(move || {
            let _ = tx.send(load(&*keys, plan));
        })
        .ok()
        .map(|_| rx)
}

#[derive(Default)]
struct Slot {
    pending: Option<Checkpoint>,
    busy: bool,
    failed: bool,
    stopped: bool,
}

struct Shared {
    slot: Mutex<Slot>,
    changed: Condvar,
}

fn lock(shared: &Shared) -> MutexGuard<'_, Slot> {
    shared
        .slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
pub struct HeadWriter {
    shared: Arc<Shared>,
    alive: bool,
}

impl HeadWriter {
    pub fn start(keys: Arc<dyn KeyStore>) -> HeadWriter {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            changed: Condvar::new(),
        });
        let worker = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-log-head".into())
            .spawn(move || run(&worker, &*keys));
        HeadWriter {
            shared,
            alive: spawned.is_ok(),
        }
    }

    pub fn submit(&self, checkpoint: Checkpoint) {
        let mut slot = lock(&self.shared);
        if !self.alive {
            slot.failed = true;
            return;
        }
        slot.pending = Some(checkpoint);
        drop(slot);
        self.shared.changed.notify_all();
    }

    pub fn failed(&self) -> bool {
        lock(&self.shared).failed
    }

    pub fn take_failed(&self) -> bool {
        std::mem::take(&mut lock(&self.shared).failed)
    }

    pub fn stop(&self) {
        lock(&self.shared).stopped = true;
        self.shared.changed.notify_all();
    }

    pub fn finish(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        let mut slot = lock(&self.shared);
        while slot.pending.is_some() || slot.busy {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            slot = self
                .shared
                .changed
                .wait_timeout(slot, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }
}

fn run(shared: &Shared, keys: &dyn KeyStore) {
    loop {
        let checkpoint = {
            let mut slot = lock(shared);
            loop {
                if let Some(checkpoint) = slot.pending.take() {
                    slot.busy = true;
                    break checkpoint;
                }
                if slot.stopped {
                    return;
                }
                slot = shared
                    .changed
                    .wait(slot)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let written = keys.write_head(&checkpoint).is_ok();
        let mut slot = lock(shared);
        slot.busy = false;
        slot.failed = !written;
        drop(slot);
        shared.changed.notify_all();
    }
}
