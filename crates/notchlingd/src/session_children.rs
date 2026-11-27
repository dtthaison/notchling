use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub const KEEP_ROWS: usize = 500;
pub const FREE_PER_HOUR: usize = 30;
const HOUR_MS: i64 = 3_600_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Child {
    pub started_by: String,
    pub tool: String,
    pub folder: String,
    pub started_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_ms: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    children: BTreeMap<String, Child>,
}

pub struct Children {
    file: Option<PathBuf>,
    book: Mutex<Book>,
    free: Mutex<HashMap<String, VecDeque<i64>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Children {
    pub fn in_memory() -> Children {
        Children {
            file: None,
            book: Mutex::new(Book::default()),
            free: Mutex::new(HashMap::new()),
        }
    }

    pub fn open(file: PathBuf) -> Children {
        let book = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Book>(&bytes).ok())
            .unwrap_or_default();
        Children {
            file: Some(file),
            book: Mutex::new(book),
            free: Mutex::new(HashMap::new()),
        }
    }

    fn save(&self, book: &Book) {
        let Some(file) = &self.file else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(book) else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    pub fn record(&self, id: &str, child: Child) {
        let mut book = lock(&self.book);
        book.children.insert(id.to_string(), child);
        while book.children.len() > KEEP_ROWS {
            let oldest = book
                .children
                .iter()
                .min_by_key(|(_, c)| c.started_ms)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(key) => book.children.remove(&key),
                None => break,
            };
        }
        self.save(&book);
    }

    pub fn get(&self, id: &str) -> Option<Child> {
        lock(&self.book).children.get(id).cloned()
    }

    pub fn stopped(&self, id: &str, at_ms: i64) {
        let mut book = lock(&self.book);
        if let Some(child) = book.children.get_mut(id) {
            child.stopped_ms = Some(at_ms);
            self.save(&book);
        }
    }

    pub fn forget(&self, id: &str) {
        let mut book = lock(&self.book);
        if book.children.remove(id).is_some() {
            self.save(&book);
        }
    }

    pub fn owned_by(&self, parent: &str, id: &str) -> Option<Child> {
        self.get(id).filter(|c| c.started_by == parent)
    }

    pub fn take_free(&self, parent: &str, now_ms: i64) -> bool {
        let mut free = lock(&self.free);
        let used = free.entry(parent.to_string()).or_default();
        while used.front().is_some_and(|at| now_ms - at >= HOUR_MS) {
            used.pop_front();
        }
        if used.len() >= FREE_PER_HOUR {
            return false;
        }
        used.push_back(now_ms);
        true
    }

    pub fn has_free(&self, parent: &str, now_ms: i64) -> bool {
        let used = lock(&self.free).get(parent).map_or(0, |used| {
            used.iter().filter(|at| now_ms - **at < HOUR_MS).count()
        });
        used < FREE_PER_HOUR
    }

    pub fn retry_after_ms(&self, parent: &str, now_ms: i64) -> i64 {
        lock(&self.free)
            .get(parent)
            .and_then(|used| used.front())
            .map_or(0, |first| (first + HOUR_MS - now_ms).max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(by: &str) -> Child {
        Child {
            started_by: by.to_string(),
            tool: "claude".to_string(),
            folder: "/work".to_string(),
            started_ms: 10,
            stopped_ms: None,
        }
    }

    #[test]
    fn a_child_is_kept_across_a_reopen() {
        let dir = std::env::temp_dir().join(format!("nl-children-{}", std::process::id()));
        let file = dir.join("children.json");
        let _ = std::fs::remove_dir_all(&dir);
        let first = Children::open(file.clone());
        first.record("job-1", child("parent"));
        first.stopped("job-1", 99);
        let again = Children::open(file);
        let kept = again.get("job-1").expect("kept");
        assert_eq!(kept.started_by, "parent");
        assert_eq!(kept.stopped_ms, Some(99));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_session_that_started_a_child_owns_it() {
        let c = Children::in_memory();
        c.record("job-1", child("parent"));
        assert!(c.owned_by("parent", "job-1").is_some());
        assert!(c.owned_by("grandparent", "job-1").is_none());
        assert!(c.owned_by("job-1", "job-1").is_none());
    }

    #[test]
    fn a_parent_gets_thirty_free_messages_an_hour() {
        let c = Children::in_memory();
        for i in 0..FREE_PER_HOUR {
            assert!(c.take_free("p", i as i64), "{i}");
        }
        assert!(!c.take_free("p", 1000));
        assert!(c.retry_after_ms("p", 1000) > 0);
        assert!(c.take_free("other", 1000));
        assert!(c.take_free("p", HOUR_MS + 5));
    }

    #[test]
    fn looking_for_room_never_uses_a_free_message() {
        let c = Children::in_memory();
        for _ in 0..(FREE_PER_HOUR * 2) {
            assert!(c.has_free("p", 10));
        }
        for i in 0..FREE_PER_HOUR {
            assert!(c.take_free("p", i as i64), "{i}");
        }
        assert!(!c.has_free("p", 1000));
        assert!(c.has_free("other", 1000));
        assert!(c.has_free("p", HOUR_MS + 5));
    }

    #[test]
    fn the_book_keeps_the_newest_rows() {
        let c = Children::in_memory();
        for i in 0..(KEEP_ROWS + 5) {
            let mut row = child("p");
            row.started_ms = i as i64;
            c.record(&format!("job-{i}"), row);
        }
        assert!(c.get("job-0").is_none());
        assert!(c.get(&format!("job-{}", KEEP_ROWS + 4)).is_some());
    }
}
