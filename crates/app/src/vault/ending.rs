use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Ending {
    open: Mutex<usize>,
    idle: Condvar,
}

fn lock(m: &Mutex<usize>) -> MutexGuard<'_, usize> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Ending {
    pub fn begin(&self) {
        *lock(&self.open) += 1;
    }

    pub fn finish(&self) {
        let mut open = lock(&self.open);
        *open = open.saturating_sub(1);
        if *open == 0 {
            self.idle.notify_all();
        }
    }

    pub fn flush(&self, wait: Duration) -> bool {
        let end = Instant::now() + wait;
        let mut open = lock(&self.open);
        while *open > 0 {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            open = self
                .idle
                .wait_timeout(open, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}
