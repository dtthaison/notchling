use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub const KEEP_MS: i64 = 12 * 3_600_000;
pub const KEEP_COUNT: usize = 5;
pub const SESSIONS_MAX: usize = 64;

#[derive(Default)]
pub struct Attempts {
    by_session: Mutex<HashMap<String, Vec<(i64, String)>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Attempts {
    pub fn note(&self, id: &str, text: &str, now_ms: i64) {
        if text.trim().is_empty() {
            return;
        }
        let mut all = lock(&self.by_session);
        all.retain(|_, list| {
            list.retain(|(at, _)| now_ms.saturating_sub(*at) < KEEP_MS);
            !list.is_empty()
        });
        if all.len() >= SESSIONS_MAX && !all.contains_key(id) {
            return;
        }
        let list = all.entry(id.to_owned()).or_default();
        list.retain(|(_, kept)| kept != text);
        list.push((now_ms, text.to_owned()));
        if list.len() > KEEP_COUNT {
            list.remove(0);
        }
    }

    pub fn recent(&self, id: &str, now_ms: i64) -> Vec<String> {
        lock(&self.by_session)
            .get(id)
            .map(|list| {
                list.iter()
                    .filter(|(at, _)| now_ms.saturating_sub(*at) < KEEP_MS)
                    .map(|(_, text)| text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn forget(&self, id: &str) {
        lock(&self.by_session).remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_keeps_its_last_few_tries_newest_last() {
        let a = Attempts::default();
        for n in 0..8 {
            a.note("job", &format!("try {n}"), 1_000 + n);
        }
        assert_eq!(
            a.recent("job", 2_000),
            ["try 3", "try 4", "try 5", "try 6", "try 7"]
        );
        assert!(a.recent("other", 2_000).is_empty());
    }

    #[test]
    fn the_same_text_tried_again_is_kept_once() {
        let a = Attempts::default();
        a.note("job", "one", 1);
        a.note("job", "two", 2);
        a.note("job", "one", 3);
        assert_eq!(a.recent("job", 10), ["two", "one"]);
    }

    #[test]
    fn old_tries_are_not_ours_any_more() {
        let a = Attempts::default();
        a.note("job", "old", 0);
        a.note("job", "new", KEEP_MS - 1);
        assert_eq!(a.recent("job", KEEP_MS), ["new"]);
        assert!(a.recent("job", KEEP_MS * 3).is_empty());
    }

    #[test]
    fn a_send_that_went_through_forgets_the_session() {
        let a = Attempts::default();
        a.note("job", "left in the box", 1);
        a.note("other", "kept", 1);
        a.forget("job");
        assert!(a.recent("job", 2).is_empty());
        assert_eq!(a.recent("other", 2), ["kept"]);
    }

    #[test]
    fn blank_text_and_a_crowd_of_sessions_cost_nothing() {
        let a = Attempts::default();
        a.note("job", "  \n", 1);
        assert!(a.recent("job", 2).is_empty());
        for n in 0..(SESSIONS_MAX + 10) {
            a.note(&format!("job-{n}"), "text", 1);
        }
        assert!(lock(&a.by_session).len() <= SESSIONS_MAX);
    }
}
