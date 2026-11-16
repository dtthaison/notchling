use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct TypingLocks {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl TypingLocks {
    pub fn new() -> TypingLocks {
        TypingLocks::default()
    }

    pub fn for_job(&self, job: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        locks.retain(|key, lock| key == job || Arc::strong_count(lock) > 1);
        locks.entry(job.to_string()).or_default().clone()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
#[path = "typing_locks_tests.rs"]
mod tests;
