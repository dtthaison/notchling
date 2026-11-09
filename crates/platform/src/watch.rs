//! Tells which files changed under some folders, so a scanner can look at just those.
//!
//! On macOS the kernel says what changed (an FSEvents stream with file-level events), so a quiet
//! folder costs nothing however many files it holds. Elsewhere, and on a Mac where the stream won't
//! start, a polling loop compares file sizes and modified times each tick, which on a real
//! `~/.claude` (tens of thousands of files) took a third of a core. Either way the changed paths
//! are reported once the folders have been quiet for the debounce time, or after the maximum
//! latency when something keeps writing; callers only see [`Batch`]es. inotify and
//! ReadDirectoryChangesW can replace the loop on Linux and Windows behind the same
//! [`Watcher::start`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::{Error, Result};

#[cfg(target_os = "macos")]
mod fsevents;

/// Folders deeper than this are not read, which also ends any loop a mount might make.
const MAX_DEPTH: usize = 32;

/// What changed since the last batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Files added, removed or changed. The native watcher also names a folder made, removed or
    /// renamed, where the polling one names the files inside it.
    pub paths: BTreeSet<PathBuf>,
    /// The caller should walk everything again: a watched folder appeared or went away, or the
    /// system lost track of some changes.
    pub rescan: bool,
}

/// How often to look, how long to wait for quiet, and how long a change may wait at most.
#[derive(Clone, Copy, Debug)]
pub struct WatchConfig {
    /// How often the polling watcher looks; the native one is told and never looks.
    pub poll: Duration,
    /// A batch is sent once nothing has changed for this long.
    pub debounce: Duration,
    /// ... or this long after its first change, whichever comes first, so something that writes
    /// during every poll (a log, a busy session) still gets its batches through. Keep it above
    /// `debounce`: below it the debounce never gets to wait.
    pub max_latency: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            poll: Duration::from_secs(1),
            debounce: Duration::from_secs(2),
            max_latency: Duration::from_secs(5),
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

/// Gathers changes into the next batch and says when it is due: after `debounce` of quiet, or
/// `max_latency` after its first change.
struct Collector {
    pending: Batch,
    quiet_since: Instant,
    /// When the batch now being collected got its first change.
    first_change: Instant,
}

impl Collector {
    fn new() -> Collector {
        let now = Instant::now();
        Collector {
            pending: Batch::default(),
            quiet_since: now,
            first_change: now,
        }
    }

    fn waiting(&self) -> bool {
        self.pending.rescan || !self.pending.paths.is_empty()
    }

    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>, rescan: bool, now: Instant) {
        let was_waiting = self.waiting();
        let mut any = rescan;
        for path in paths {
            // A path already pending changing again still counts: the folder isn't quiet.
            any = true;
            self.pending.paths.insert(path);
        }
        if !any {
            return;
        }
        // While a batch is already collecting only the quiet clock restarts.
        if !was_waiting {
            self.first_change = now;
        }
        self.pending.rescan |= rescan;
        self.quiet_since = now;
    }

    /// When the batch being collected should go out; `None` while there is none.
    fn due_at(&self, config: &WatchConfig) -> Option<Instant> {
        self.waiting().then(|| {
            (self.quiet_since + config.debounce).min(self.first_change + config.max_latency)
        })
    }

    fn take_if_due(&mut self, now: Instant, config: &WatchConfig) -> Option<Batch> {
        let due = self.due_at(config).is_some_and(|at| now >= at);
        due.then(|| std::mem::take(&mut self.pending))
    }
}

/// Ends the watcher's thread when dropped.
enum Stop {
    /// Dropping the sender wakes the polling loop's wait at once.
    Poll(Sender<()>),
    /// The stream's callback holds the only sender of the batching thread's events, so stopping
    /// the stream ends the thread too.
    #[cfg(target_os = "macos")]
    Native(fsevents::Stream),
}

/// A running watcher. Dropping it stops the thread.
pub struct Watcher {
    stop: Option<Stop>,
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
        #[cfg(target_os = "macos")]
        if let Some(started) = start_native(&existing, config) {
            return Ok(started);
        }
        start_polling(existing, config)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        match self.stop.take() {
            Some(Stop::Poll(sender)) => drop(sender),
            // Stops the stream; once this returns no callback runs and its sender is gone.
            #[cfg(target_os = "macos")]
            Some(Stop::Native(stream)) => drop(stream),
            None => {}
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `folders` all exist and there is at least one.
fn start_polling(folders: Vec<PathBuf>, config: WatchConfig) -> Result<(Watcher, Receiver<Batch>)> {
    // Taken here, not on the thread, so a change right after `start` returns is seen.
    let first = scan(&folders);
    let (stop_tx, stop_rx) = mpsc::channel();
    let (batch_tx, batch_rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("notchling-watch".into())
        .spawn(move || poll(folders, first, config, stop_rx, batch_tx))
        .map_err(Error::Io)?;
    Ok((
        Watcher {
            stop: Some(Stop::Poll(stop_tx)),
            thread: Some(thread),
        },
        batch_rx,
    ))
}

fn poll(
    folders: Vec<PathBuf>,
    first: Snapshot,
    config: WatchConfig,
    stop: Receiver<()>,
    out: Sender<Batch>,
) {
    let mut last = first;
    // Every folder existed when `start` filtered them.
    let mut roots_present = vec![true; folders.len()];
    let mut collector = Collector::new();
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
        let now = Instant::now();
        collector.add(changed, roots_changed, now);
        if let Some(batch) = collector.take_if_due(now, &config) {
            if out.send(batch).is_err() {
                return;
            }
        }
    }
}

/// How long FSEvents gathers events before calling back. The first change after a quiet spell is
/// delivered at once (`NoDefer`); this only groups the ones that follow it, so it stays well under
/// the debounce, which is what decides when a batch goes out.
#[cfg(target_os = "macos")]
fn stream_latency(config: &WatchConfig) -> Duration {
    (config.debounce / 4).clamp(Duration::from_millis(10), Duration::from_millis(500))
}

/// The FSEvents watcher; `None` when the stream won't start, so the caller polls instead.
#[cfg(target_os = "macos")]
fn start_native(folders: &[PathBuf], config: WatchConfig) -> Option<(Watcher, Receiver<Batch>)> {
    let (event_tx, event_rx) = mpsc::channel();
    let stream = fsevents::Stream::start(
        fsevents::real_roots(folders),
        stream_latency(&config),
        event_tx,
    )?;
    let (batch_tx, batch_rx) = mpsc::channel();
    let folders = folders.to_vec();
    // On failure the stream drops here and stops.
    let thread = std::thread::Builder::new()
        .name("notchling-watch".into())
        .spawn(move || batch_events(folders, config, event_rx, batch_tx))
        .ok()?;
    Some((
        Watcher {
            stop: Some(Stop::Native(stream)),
            thread: Some(thread),
        },
        batch_rx,
    ))
}

/// Turns the stream's events into batches. Sleeps until an event comes or a batch is due, so it
/// costs nothing while the folders are quiet.
#[cfg(target_os = "macos")]
fn batch_events(
    folders: Vec<PathBuf>,
    config: WatchConfig,
    events: Receiver<fsevents::Event>,
    out: Sender<Batch>,
) {
    // Every folder existed when `start` filtered them.
    let mut roots_present = vec![true; folders.len()];
    let mut collector = Collector::new();
    loop {
        let got = match collector.due_at(&config) {
            Some(at) => events.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => events.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(event) => {
                // A watched folder coming or going is a rescan, as with polling; a few stats per
                // event, never while quiet.
                let now_present: Vec<bool> = folders.iter().map(|f| f.is_dir()).collect();
                let roots_changed = now_present != roots_present;
                roots_present = now_present;
                collector.add(event.paths, event.rescan || roots_changed, Instant::now());
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The stream stopped: the watcher was dropped.
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some(batch) = collector.take_if_due(Instant::now(), &config) {
            if out.send(batch).is_err() {
                return;
            }
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
        max_latency: Duration::from_secs(2),
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
    fn steady_writes_still_produce_a_batch_within_the_max_latency() {
        // Something writes during every poll, so the folder is never quiet for the debounce.
        let dir = TempDir::new();
        let file = dir.path().join("busy.log");
        fs::write(&file, b"x").unwrap();
        let config = WatchConfig {
            poll: Duration::from_millis(20),
            debounce: Duration::from_millis(300),
            max_latency: Duration::from_millis(600),
        };
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], config).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let (file, stop) = (file.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut len = 1;
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    len += 1;
                    fs::write(&file, vec![b'x'; len]).unwrap();
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };
        let started = Instant::now();
        let first = rx.recv_timeout(Duration::from_millis(1500));
        let waited = started.elapsed();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        writer.join().unwrap();
        let batch = first.expect("a batch although the writes never paused");
        assert!(batch.paths.contains(&file));
        assert!(waited < Duration::from_millis(1500), "{waited:?}");
    }

    #[test]
    fn the_max_latency_does_not_cut_a_debounce_short() {
        // One change, then quiet: sent after the debounce, not at once.
        let dir = TempDir::new();
        let config = WatchConfig {
            poll: Duration::from_millis(20),
            debounce: Duration::from_millis(300),
            max_latency: Duration::from_secs(5),
        };
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], config).unwrap();
        fs::write(dir.path().join("f"), b"x").unwrap();
        let started = Instant::now();
        rx.recv_timeout(WAIT).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(280));
    }

    #[test]
    fn dropping_the_watcher_stops_it_promptly() {
        let dir = TempDir::new();
        let slow = WatchConfig {
            poll: Duration::from_secs(30),
            debounce: Duration::from_secs(30),
            max_latency: Duration::from_secs(30),
        };
        let (watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], slow).unwrap();
        let started = Instant::now();
        drop(watcher);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(rx.recv().is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_the_kernel_says_what_changed() {
        let dir = TempDir::new();
        let (watcher, _rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        assert!(matches!(watcher.stop, Some(Stop::Native(_))));
    }

    #[test]
    fn a_burst_is_coalesced_into_one_batch() {
        let dir = TempDir::new();
        let (_watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], FAST).unwrap();
        let files: Vec<PathBuf> = (0..20).map(|i| dir.path().join(format!("f{i}"))).collect();
        for f in &files {
            fs::write(f, b"x").unwrap();
        }
        let batch = rx.recv_timeout(WAIT).unwrap();
        assert!(files.iter().all(|f| batch.paths.contains(f)), "{batch:?}");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_vanished_folder_asks_for_a_rescan_and_the_others_still_report() {
        let dir = TempDir::new();
        let (gone, kept) = (dir.path().join("gone"), dir.path().join("kept"));
        fs::create_dir(&gone).unwrap();
        fs::create_dir(&kept).unwrap();
        fs::write(gone.join("f"), b"x").unwrap();
        let (_watcher, rx) = Watcher::start(vec![gone.clone(), kept.clone()], FAST).unwrap();
        fs::remove_dir_all(&gone).unwrap();
        let mut rescan = false;
        while !rescan {
            rescan = rx.recv_timeout(WAIT).unwrap().rescan;
        }
        let file = kept.join("after.txt");
        fs::write(&file, b"x").unwrap();
        let batch = rx.recv_timeout(WAIT).unwrap();
        assert!(batch.paths.contains(&file), "{batch:?}");
        assert!(!batch.rescan);
    }

    #[test]
    fn dropping_the_watcher_stops_delivery() {
        // A change still waiting for its debounce when the watcher goes is never sent.
        let dir = TempDir::new();
        let config = WatchConfig {
            debounce: Duration::from_secs(30),
            max_latency: Duration::from_secs(60),
            ..FAST
        };
        let (watcher, rx) = Watcher::start(vec![dir.path().to_path_buf()], config).unwrap();
        fs::write(dir.path().join("f"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        drop(watcher);
        assert!(started.elapsed() < Duration::from_secs(5));
        fs::write(dir.path().join("g"), b"x").unwrap();
        assert_eq!(rx.recv_timeout(WAIT), Err(RecvTimeoutError::Disconnected));
    }

    #[test]
    fn the_polling_fallback_reports_changes_and_a_vanished_folder() {
        // Tested on its own too, since on a Mac `start` only polls when FSEvents fails.
        let dir = TempDir::new();
        let watched = dir.path().join("w");
        fs::create_dir(&watched).unwrap();
        let (_watcher, rx) = start_polling(vec![watched.clone()], FAST).unwrap();
        let file = watched.join("f.txt");
        fs::write(&file, b"hi").unwrap();
        assert!(rx.recv_timeout(WAIT).unwrap().paths.contains(&file));
        fs::remove_dir_all(&watched).unwrap();
        assert!(rx.recv_timeout(WAIT).unwrap().rescan);
    }

    #[test]
    fn a_path_changing_again_keeps_the_batch_waiting_for_quiet() {
        let config = WatchConfig {
            poll: Duration::from_millis(10),
            debounce: Duration::from_millis(100),
            max_latency: Duration::from_secs(1),
        };
        let t0 = Instant::now();
        let mut c = Collector::new();
        assert_eq!(c.due_at(&config), None);
        c.add([PathBuf::from("/a")], false, t0);
        c.add([PathBuf::from("/a")], false, t0 + Duration::from_millis(80));
        assert_eq!(
            c.due_at(&config),
            Some(t0 + Duration::from_millis(180)),
            "the second change restarts the quiet clock"
        );
        assert!(c
            .take_if_due(t0 + Duration::from_millis(150), &config)
            .is_none());
        // Something writing all the time still goes out at the max latency.
        for ms in (100..1200).step_by(50) {
            c.add([PathBuf::from("/a")], false, t0 + Duration::from_millis(ms));
        }
        assert_eq!(c.due_at(&config), Some(t0 + Duration::from_secs(1)));
        let batch = c.take_if_due(t0 + Duration::from_secs(1), &config).unwrap();
        assert_eq!(batch.paths.len(), 1);
        assert_eq!(c.due_at(&config), None);
        c.add(Vec::new(), false, t0);
        assert_eq!(c.due_at(&config), None, "nothing new is not a change");
    }
}
