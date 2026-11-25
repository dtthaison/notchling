use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::{Error, Result};

#[cfg(target_os = "macos")]
mod fsevents;

const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub paths: BTreeSet<PathBuf>,
    pub rescan: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct WatchConfig {
    pub poll: Duration,
    pub debounce: Duration,
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

struct Collector {
    pending: Batch,
    quiet_since: Instant,
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
            any = true;
            self.pending.paths.insert(path);
        }
        if !any {
            return;
        }
        if !was_waiting {
            self.first_change = now;
        }
        self.pending.rescan |= rescan;
        self.quiet_since = now;
    }

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

enum Stop {
    Poll(Sender<()>),
    #[cfg(target_os = "macos")]
    Native(fsevents::Stream),
}

pub struct Watcher {
    stop: Option<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
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
            #[cfg(target_os = "macos")]
            Some(Stop::Native(stream)) => drop(stream),
            None => {}
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn start_polling(folders: Vec<PathBuf>, config: WatchConfig) -> Result<(Watcher, Receiver<Batch>)> {
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

#[cfg(target_os = "macos")]
fn stream_latency(config: &WatchConfig) -> Duration {
    (config.debounce / 4).clamp(Duration::from_millis(10), Duration::from_millis(500))
}

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

#[cfg(target_os = "macos")]
fn batch_events(
    folders: Vec<PathBuf>,
    config: WatchConfig,
    events: Receiver<fsevents::Event>,
    out: Sender<Batch>,
) {
    let mut roots_present = vec![true; folders.len()];
    let mut collector = Collector::new();
    loop {
        let got = match collector.due_at(&config) {
            Some(at) => events.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => events.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(event) => {
                let now_present: Vec<bool> = folders.iter().map(|f| f.is_dir()).collect();
                let roots_changed = now_present != roots_present;
                roots_present = now_present;
                collector.add(event.paths, event.rescan || roots_changed, Instant::now());
            }
            Err(RecvTimeoutError::Timeout) => {}
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
