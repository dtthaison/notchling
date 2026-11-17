use crate::chat::git::{Why, EVENT_FLOOR_MS};

pub const SLOW_EVERY_MS: i64 = 15_000;
pub const PENDING_AGAIN_MS: i64 = 2_000;
const FRESH_MAX: usize = 2;

#[derive(Debug, Default)]
pub struct SlowGate {
    last_ms: Option<i64>,
    folder: Option<String>,
    inflight: bool,
    retry_at: Option<i64>,
}

impl SlowGate {
    pub fn ask(&mut self, why: Why, folder: Option<&str>, visible: bool, now_ms: i64) -> bool {
        let Some(folder) = folder.filter(|f| !f.is_empty()) else {
            return false;
        };
        if !visible || self.inflight {
            return false;
        }
        let since = self.last_ms.map_or(i64::MAX, |last| now_ms - last);
        let changed = self.folder.as_deref() != Some(folder);
        let retry_due = self.retry_at.is_some_and(|at| now_ms >= at);
        let go = match why {
            Why::Opened | Why::Folder => changed || since >= EVENT_FLOOR_MS,
            Why::TurnEnded => since >= EVENT_FLOOR_MS,
            Why::Tick => changed || since >= SLOW_EVERY_MS || retry_due,
        };
        if go {
            self.last_ms = Some(now_ms);
            self.folder = Some(folder.to_owned());
            self.inflight = true;
            self.retry_at = None;
        }
        go
    }

    pub fn done(&mut self, pending: bool, now_ms: i64) {
        self.inflight = false;
        self.retry_at = pending.then_some(now_ms + PENDING_AGAIN_MS);
    }

    pub fn reset(&mut self) {
        self.last_ms = None;
        self.retry_at = None;
    }
}

pub fn fresh_after<T: PartialEq>(before: &[(String, T)], after: &[(String, T)]) -> Vec<String> {
    after
        .iter()
        .filter(|(path, value)| {
            before
                .iter()
                .find(|(was, _)| was == path)
                .is_some_and(|(_, was)| was != value)
        })
        .map(|(path, _)| path.clone())
        .take(FRESH_MAX)
        .collect()
}

#[cfg(test)]
#[path = "git_gates_tests.rs"]
mod tests;
