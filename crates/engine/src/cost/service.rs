use super::check::{analyse, Options, Report};
use super::ledger::Ledger;
use super::summary::Window;
use super::tracker::{Ranged, Tracker};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const SCAN_EVERY: f64 = 20.0;
pub const LEDGER_SLICE: Duration = Duration::from_secs(3);
pub const LEDGER_EVERY: f64 = 300.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    pub report: Report,
    pub complete: bool,
    pub scanned: Option<f64>,
    pub calls: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub scanned: Option<f64>,
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
    ledger: Mutex<Option<Ledger>>,
    published: Mutex<(Option<f64>, Ranged)>,
    scanning: AtomicBool,
    filling: AtomicBool,
    settled: AtomicBool,
    last_started: Mutex<Option<f64>>,
    last_kept: Mutex<Option<f64>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Scanning<'a>(&'a AtomicBool);

impl Drop for Scanning<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl CostService {
    pub fn new(tracker: Tracker) -> Arc<CostService> {
        CostService::with_ledger(tracker, None)
    }

    pub fn with_ledger(tracker: Tracker, ledger: Option<Ledger>) -> Arc<CostService> {
        Arc::new(CostService {
            tracker: Mutex::new(tracker),
            ledger: Mutex::new(ledger),
            published: Mutex::new((None, Ranged::default())),
            scanning: AtomicBool::new(false),
            filling: AtomicBool::new(false),
            settled: AtomicBool::new(false),
            last_started: Mutex::new(None),
            last_kept: Mutex::new(None),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        let published = lock(&self.published);
        Snapshot {
            scanned: published.0,
            scanning: self.scanning.load(Ordering::SeqCst),
            ranged: published.1.clone(),
        }
    }

    pub fn request(self: &Arc<Self>, now: f64) -> Snapshot {
        let due = lock(&self.last_started).is_none_or(|t| now - t >= SCAN_EVERY);
        if due && !self.scanning.load(Ordering::SeqCst) {
            let service = self.clone();
            let _ = std::thread::Builder::new()
                .name("notchling-cost".into())
                .spawn(move || service.refresh(now));
        }
        self.snapshot()
    }

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

    pub fn keep(self: &Arc<Self>, now: f64) {
        let settled = self.settled.load(Ordering::SeqCst);
        let due = !settled || lock(&self.last_kept).is_none_or(|t| now - t >= LEDGER_EVERY);
        if !due || self.filling.load(Ordering::SeqCst) {
            return;
        }
        *lock(&self.last_kept) = Some(now);
        let service = self.clone();
        let _ = std::thread::Builder::new()
            .name("notchling-usage".into())
            .spawn(move || {
                service.fill_slice(now);
            });
    }

    pub fn fill_slice(&self, now: f64) -> bool {
        if self.filling.swap(true, Ordering::SeqCst) {
            return false;
        }
        let _filling = Scanning(&self.filling);
        let Some(complete) = lock(&self.ledger)
            .as_mut()
            .map(|ledger| ledger.refresh(now, Some(LEDGER_SLICE)).complete())
        else {
            return true;
        };
        self.settled.store(complete, Ordering::SeqCst);
        complete
    }

    pub fn check(&self, options: Options, budget: Duration) -> Option<Checked> {
        let mut guard = lock(&self.ledger);
        let ledger = guard.as_mut()?;
        let progress = ledger.refresh(options.now, Some(budget));
        let from = options.now - f64::from(options.days) * 86_400.0;
        let window = ledger.calls_since(from);
        let mut report = analyse(&window, ledger.catalog(), &options);
        report.guard = ledger.guard_since(from, options.now, options.days);
        Some(Checked {
            report,
            complete: progress.complete(),
            scanned: ledger.scanned(),
            calls: ledger.call_count(),
        })
    }

    pub fn save(&self, now: f64) {
        lock(&self.tracker).save_now(now);
    }
}
