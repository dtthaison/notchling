use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) const GRACE_MS: i64 = 5_000;

pub(crate) type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Default)]
struct Entry {
    live: usize,
    since: i64,
    left: Option<i64>,
    reported: bool,
}

pub(crate) struct Presence {
    grace_ms: i64,
    clock: Clock,
    entries: Mutex<HashMap<String, Entry>>,
}

pub(crate) struct PresenceGuard {
    presence: Arc<Presence>,
    key: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Entry {
    fn present(&self, now: i64, grace_ms: i64) -> bool {
        self.live > 0
            || self
                .left
                .is_some_and(|left| now.saturating_sub(left) < grace_ms)
    }
}

impl Presence {
    pub(crate) fn new(grace_ms: i64, clock: Clock) -> Arc<Presence> {
        Arc::new(Presence {
            grace_ms,
            clock,
            entries: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn enter(self: &Arc<Self>, key: &str, now: i64) -> PresenceGuard {
        let mut entries = lock(&self.entries);
        let entry = entries.entry(key.to_string()).or_default();
        if !entry.present(now, self.grace_ms) {
            entry.since = now;
            entry.reported = false;
        }
        entry.live += 1;
        entry.left = None;
        PresenceGuard {
            presence: self.clone(),
            key: key.to_string(),
        }
    }

    fn leave(&self, key: &str) {
        let now = (self.clock)();
        if let Some(entry) = lock(&self.entries).get_mut(key) {
            entry.live = entry.live.saturating_sub(1);
            if entry.live == 0 {
                entry.left = Some(now);
            }
        }
    }

    pub(crate) fn forget(&self, key: &str) {
        lock(&self.entries).remove(key);
    }

    pub(crate) fn present(&self, key: &str, now: i64) -> bool {
        lock(&self.entries)
            .get(key)
            .is_some_and(|entry| entry.present(now, self.grace_ms))
    }

    pub(crate) fn since(&self, key: &str) -> Option<i64> {
        lock(&self.entries).get(key).map(|entry| entry.since)
    }

    pub(crate) fn sweep(&self, now: i64) -> Vec<(String, bool)> {
        let mut entries = lock(&self.entries);
        let mut changes = Vec::new();
        for (key, entry) in entries.iter_mut() {
            let here = entry.present(now, self.grace_ms);
            if here != entry.reported {
                entry.reported = here;
                changes.push((key.clone(), here));
            }
        }
        entries.retain(|_, entry| entry.reported || entry.present(now, self.grace_ms));
        changes.sort();
        changes
    }
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        self.presence.leave(&self.key);
    }
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
