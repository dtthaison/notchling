//! The tracker as a service for the daemon: a scan runs on a thread of its own when a request finds
//! the last one old, and a request answers at once with the summaries the last scan published
//! (`scanning` says one is running), so a first scan over several GB never holds a caller past its
//! timeout. Nothing runs on its own: no timer, no watcher, only a request starts a scan.

use super::summary::Window;
use super::tracker::{Ranged, Tracker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// A request starts a scan when the last one began this long ago.
pub const SCAN_EVERY: f64 = 20.0;

/// What a request gets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// When the last finished scan began (seconds since 1970); `None` before the first.
    pub scanned: Option<f64>,
    /// A scan is running now; ask again for its result.
    pub scanning: bool,
    pub ranged: Ranged,
}

impl Snapshot {
    pub fn summary(&self, window: Window) -> Option<&super::summary::Summary> {
        self.ranged.overall.get(&window)
    }
}

pub struct CostService {
    tracker: Mutex<Tracker>,
    published: Mutex<(Option<f64>, Ranged)>,
    scanning: AtomicBool,
    last_started: Mutex<Option<f64>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Clears `scanning` however the scan ends.
struct Scanning<'a>(&'a AtomicBool);

impl Drop for Scanning<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl CostService {
    pub fn new(tracker: Tracker) -> Arc<CostService> {
        Arc::new(CostService {
            tracker: Mutex::new(tracker),
            published: Mutex::new((None, Ranged::default())),
            scanning: AtomicBool::new(false),
            last_started: Mutex::new(None),
        })
    }

    /// The last published summaries; starts nothing.
    pub fn snapshot(&self) -> Snapshot {
        let published = lock(&self.published);
        Snapshot {
            scanned: published.0,
            scanning: self.scanning.load(Ordering::SeqCst),
            ranged: published.1.clone(),
        }
    }

    /// Answers with what is published, and starts a scan on a thread when none runs and the last
    /// one is old (or there was none).
    pub fn request(self: &Arc<Self>, now: f64) -> Snapshot {
        let due = lock(&self.last_started).is_none_or(|t| now - t >= SCAN_EVERY);
        if due && !self.scanning.load(Ordering::SeqCst) {
            let service = self.clone();
            // A thread that can't start leaves the old summaries; the next request tries again.
            let _ = std::thread::Builder::new()
                .name("notchling-cost".into())
                .spawn(move || service.refresh(now));
        }
        self.snapshot()
    }

    /// Scans now, on this thread, and publishes. Does nothing when a scan is already running.
    pub fn refresh(&self, now: f64) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let _scanning = Scanning(&self.scanning);
        *lock(&self.last_started) = Some(now);
        let mut tracker = lock(&self.tracker);
        tracker.refresh(now);
        let ranged = tracker.ranged(now);
        drop(tracker);
        *lock(&self.published) = (Some(now), ranged);
    }

    /// Writes counts not saved yet (the daemon calls it as it stops). Waits for a scan.
    pub fn save(&self, now: f64) {
        lock(&self.tracker).save_now(now);
    }
}
