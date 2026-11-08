//! Tells which files changed under some folders, so a scanner can look at just those.
//!
//! This is a polling watcher on every OS: it compares file sizes and modified times each tick and
//! reports the changed paths once the folders have been quiet for the debounce time. A native
//! watcher (FSEvents, inotify, ReadDirectoryChangesW) can replace the loop behind the same
//! [`Watcher::start`] when scanning cost shows up; callers only see [`Batch`]es.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::{Error, Result};

/// Folders deeper than this are not read, which also ends any loop a mount might make.
const MAX_DEPTH: usize = 32;

/// What changed since the last batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Files added, removed or changed.
    pub paths: BTreeSet<PathBuf>,
    /// The caller should walk everything again: a watched folder appeared or went away.
    pub rescan: bool,
}

/// How often to look, and how long to wait for quiet.
#[derive(Clone, Copy, Debug)]
pub struct WatchConfig {
    pub poll: Duration,
    pub debounce: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            poll: Duration::from_secs(1),
            debounce: Duration::from_secs(2),
        }
    }
}

type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

/// Reads every file under `dir`; symlinks are not followed.
fn scan_into(dir: &Path, depth: usize, out: &mut Snapshot) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            scan_into(&entry.path(), depth + 1, out);
        } else if meta.is_file() {
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.insert(entry.path(), (modified, meta.len()));
        }
    }
}

fn scan(folders: &[PathBuf]) -> Snapshot {
    let mut out = Snapshot::new();
    for folder in folders {
        scan_into(folder, 0, &mut out);
    }
    out
}

/// Paths added, removed or changed between two snapshots.
fn diff(old: &Snapshot, new: &Snapshot) -> BTreeSet<PathBuf> {
    let mut changed = BTreeSet::new();
    for (path, stamp) in new {
        if old.get(path) != Some(stamp) {
            changed.insert(path.clone());
        }
    }
    for path in old.keys() {
        if !new.contains_key(path) {
            changed.insert(path.clone());
        }
    }
    changed
}

/// A running watcher. Dropping it stops the thread.
pub struct Watcher {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts watching the folders that exist now. Fails when none do. Batches arrive on the
    /// returned channel; dropping the receiver also ends the watch.
    pub fn start(folders: Vec<PathBuf>, config: WatchConfig) -> Result<(Watcher, Receiver<Batch>)> {
        let existing: Vec<PathBuf> = folders.into_iter().filter(|f| f.is_dir()).collect();
        if existing.is_empty() {
            return Err(Error::System("no folder to watch exists".into()));
        }
        // Taken here, not on the thread, so a change right after `start` returns is seen.
        let first = scan(&existing);
        let (stop_tx, stop_rx) = mpsc::channel();
        let (batch_tx, batch_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("notchling-watch".into())
            .spawn(move || run(existing, first, config, stop_rx, batch_tx))
            .map_err(Error::Io)?;
        Ok((
            Watcher {
                stop: Some(stop_tx),
                thread: Some(thread),
            },
            batch_rx,
        ))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Dropping the sender wakes the thread's wait at once.
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    folders: Vec<PathBuf>,
    first: Snapshot,
    config: WatchConfig,
    stop: Receiver<()>,
    out: Sender<Batch>,
) {
    let mut last = first;
    // Every folder existed when `start` filtered them.
    let mut roots_present = vec![true; folders.len()];
    let mut pending = Batch::default();
    let mut quiet_since = Instant::now();
    loop {
        match stop.recv_timeout(config.poll) {
            Err(RecvTimeoutError::Timeout) => {}
            _ => return,
        }
        let now_present: Vec<bool> = folders.iter().map(|f| f.is_dir()).collect();
        let roots_changed = now_present != roots_present;
        roots_present = now_present;
        let next = scan(&folders);
        let changed = diff(&last, &next);
        last = next;
        if roots_changed || !changed.is_empty() {
            pending.paths.extend(changed);
            pending.rescan |= roots_changed;
            quiet_since = Instant::now();
        }
        let waiting = pending.rescan || !pending.paths.is_empty();
        let due = waiting && quiet_since.elapsed() >= config.debounce;
        if due && out.send(std::mem::take(&mut pending)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdir::TempDir;

    const FAST: WatchConfig = WatchConfig {
        poll: Duration::from_millis(20),
        debounce: Duration::from_millis(80),
    };
    const WAIT: Duration = Duration::from_secs(5);

    fn stamp(secs: u64, len: u64) -> (SystemTime, u64) {
        (SystemTime::UNIX_EPOCH + Duration::from_secs(secs), len)
    }

    #[test]
    fn diff_reports_added_removed_and_changed_only() {
        let mut old = Snapshot::new();
        old.insert("/a".into(), stamp(1, 1));
        old.insert("/b".into(), stamp(1, 1));
        old.insert("/c".into(), stamp(1, 1));
        let mut new = Snapshot::new();
        new.insert("/a".into(), stamp(1, 1));
        new.insert("/b".into(), stamp(2, 1));
        new.insert("/d".into(), stamp(1, 1));
        let got: Vec<_> = diff(&old, &new).into_iter().collect();
        assert_eq!(
            got,
            vec![
                PathBuf::from("/b"),
                PathBuf::from("/c"),
                PathBuf::from("/d")
            ]
        );
    }

    #[test]
    fn no_folder_is_an_error() {
        let dir = TempDir::new();
        assert!(Watcher::start(vec![dir.path().join("missing")], FAST).is_err());
    }

    #[test]
    fn reports_a_new_file_in_a_subfolder() {
        let dir = TempDir::new();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        let file = dir.path().join("sub/new.txt");
        fs::write(&file, b"hi").unwrap();
        let batch = rx.recv_timeout(WAIT).unwrap();
        assert!(batch.paths.contains(&file));
        assert!(!batch.rescan);
    }

    #[test]
    fn reports_a_change_and_a_removal() {
        let dir = TempDir::new();
        let file = dir.path().join("f.txt");
        fs::write(&file, b"one").unwrap();
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        fs::write(&file, b"longer content").unwrap();
        assert!(rx.recv_timeout(WAIT).unwrap().paths.contains(&file));
        fs::remove_file(&file).unwrap();
        assert!(rx.recv_timeout(WAIT).unwrap().paths.contains(&file));
    }

    #[test]
    fn a_burst_of_writes_is_reported_together() {
        let dir = TempDir::new();
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        let files: Vec<PathBuf> = (0..3).map(|i| dir.path().join(format!("f{i}"))).collect();
        for f in &files {
            fs::write(f, b"x").unwrap();
        }
        let mut seen = BTreeSet::new();
        while seen.len() < files.len() {
            seen.extend(rx.recv_timeout(WAIT).unwrap().paths);
        }
        assert!(files.iter().all(|f| seen.contains(f)));
    }

    #[test]
    fn nothing_is_sent_while_nothing_changes() {
        let dir = TempDir::new();
        fs::write(dir.path().join("f"), b"x").unwrap();
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(400)).is_err());
    }

    #[test]
    fn a_vanished_watched_folder_asks_for_a_rescan() {
        let dir = TempDir::new();
        let watched = dir.path().join("w");
        fs::create_dir(&watched).unwrap();
        let (_watcher, rx) = Watcher::start(vec![watched.clone()], FAST).unwrap();
        fs::remove_dir_all(&watched).unwrap();
        assert!(rx.recv_timeout(WAIT).unwrap().rescan);
    }

    #[test]
    fn dropping_the_watcher_stops_it_promptly() {
        let dir = TempDir::new();
        let slow = WatchConfig {
            poll: Duration::from_secs(30),
            debounce: Duration::from_secs(30),
        };
        let (watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], slow).unwrap();
        let started = Instant::now();
        drop(watcher);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(rx.recv().is_err());
    }
}
