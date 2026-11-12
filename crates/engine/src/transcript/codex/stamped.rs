//! What was read from a rollout, kept until the file changes (its size or time). The notch's
//! poll (every 2 s) and Chat's reloads list up to 30 rollouts each time and read each one's tail
//! (`turn_open`, 256 KB) and, until it has a title, its head (`meta`, up to 3 MB): a few MB read and
//! scanned per poll, nearly always from files that hadn't changed.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

/// A file's size and time, as far as a reader can tell whether it changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    /// The file's stamp now; `None` when it can't be looked at.
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }

    #[cfg(test)]
    pub fn new(len: u64) -> Stamp {
        Stamp {
            len,
            modified: None,
        }
    }
}

/// Answers kept per path with the stamp they were read at. Past [`Stamped::MAX`] paths it starts
/// over, so rollouts that left the list don't pile up.
pub struct Stamped<T> {
    kept: Mutex<Option<HashMap<String, (Stamp, T)>>>,
}

impl<T: Clone> Stamped<T> {
    pub const MAX: usize = 512;

    pub const fn new() -> Stamped<T> {
        Stamped {
            kept: Mutex::new(None),
        }
    }

    /// The answer for `path` at `stamp`: the kept one when the stamp is the same, else `read()`,
    /// kept for next time.
    pub fn get(&self, path: &str, stamp: Stamp, read: impl FnOnce() -> T) -> T {
        {
            let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, value)) = kept.as_ref().and_then(|k| k.get(path)) {
                if *at == stamp {
                    return value.clone();
                }
            }
        }
        // Read without the lock: another path's read needn't wait for this one.
        let value = read();
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        let map = kept.get_or_insert_with(HashMap::new);
        if map.len() >= Self::MAX && !map.contains_key(path) {
            map.clear();
        }
        map.insert(path.to_owned(), (stamp, value.clone()));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn an_unchanged_file_is_read_once_and_a_changed_one_again() {
        let kept: Stamped<u32> = Stamped::new();
        let reads = Cell::new(0);
        let read = |v: u32| {
            reads.set(reads.get() + 1);
            v
        };
        assert_eq!(kept.get("/a", Stamp::new(10), || read(1)), 1);
        assert_eq!(kept.get("/a", Stamp::new(10), || read(2)), 1, "unchanged");
        assert_eq!(reads.get(), 1);
        assert_eq!(kept.get("/a", Stamp::new(11), || read(3)), 3, "it grew");
        assert_eq!(kept.get("/b", Stamp::new(11), || read(4)), 4, "per path");
        assert_eq!(reads.get(), 3);
    }

    #[test]
    fn it_starts_over_past_its_size() {
        let kept: Stamped<usize> = Stamped::new();
        for n in 0..Stamped::<usize>::MAX {
            kept.get(&n.to_string(), Stamp::new(1), || n);
        }
        let len = |k: &Stamped<usize>| {
            let kept = k.kept.lock().unwrap_or_else(|e| e.into_inner());
            kept.as_ref().map_or(0, HashMap::len)
        };
        assert_eq!(len(&kept), Stamped::<usize>::MAX);
        kept.get("one more", Stamp::new(1), || 0);
        assert_eq!(len(&kept), 1);
    }

    #[test]
    fn a_real_file_changes_its_stamp_when_it_grows() {
        let dir = std::env::temp_dir().join(format!("nl-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let file = dir.join("r.jsonl");
        std::fs::write(&file, "a\n").ok();
        let before = Stamp::of(&file);
        assert!(before.is_some());
        std::fs::write(&file, "a\nb\n").ok();
        assert_ne!(Stamp::of(&file), before);
        assert_eq!(Stamp::of(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
