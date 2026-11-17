use crate::outbox_anchor::{count_anchored, count_row, is_clear, message_of};
use crate::outbox_courier::{live_files, real_clock, Clock, JobFiles};
use crate::outbox_postman::{Baseline, Verifier};
use notchling_engine::outbox::{
    line_within, queued_verdict, verdict, Anchor, QueuedVerdict, Row, Verdict, GONE_AFTER_MS,
    QUEUE_LINE_MS, VERIFY_LOOK_MS, VERIFY_WATCH_MS,
};
use notchling_engine::sessions::{Agent, Roots};
use notchling_engine::transcript::message::Message;
use notchling_engine::transcript::queue_state::{queue_lists, QueueState};
use notchling_engine::transcript::sent::Seen;
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

pub const SINCE_BEFORE_MS: i64 = 2_000;
const SLICE_MS: i64 = 250;
const WATCH: WatchConfig = WatchConfig {
    poll: Duration::from_secs(1),
    debounce: Duration::from_millis(100),
    max_latency: Duration::from_millis(500),
};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
pub struct SafeModes {
    failed: Mutex<HashSet<String>>,
}

impl SafeModes {
    pub fn is_safe(&self, session: &str, queue: &QueueState) -> bool {
        queue.unknown > 0 || lock(&self.failed).contains(session)
    }

    pub fn note(&self, session: &str, line_within: bool) {
        let mut failed = lock(&self.failed);
        if line_within {
            failed.remove(session);
        } else {
            failed.insert(session.to_string());
        }
    }
}

pub(crate) trait Waits {
    fn next(&mut self, deadline_ms: i64);
}

pub(crate) trait Pause: Send + Sync {
    fn watch(&self, file: &Path, any_jsonl: bool) -> Box<dyn Waits + '_>;
    fn sleep(&self, deadline_ms: i64);
}

struct RealPause {
    clock: Clock,
    closed: Arc<AtomicBool>,
}

struct RealWaits<'a> {
    pause: &'a RealPause,
    file: PathBuf,
    any_jsonl: bool,
    batches: Option<(Watcher, Receiver<Batch>)>,
}

impl RealPause {
    fn left(&self, deadline_ms: i64) -> Option<Duration> {
        let left = deadline_ms - (self.clock)();
        if left <= 0 || self.closed.load(Ordering::SeqCst) {
            return None;
        }
        Some(Duration::from_millis(left.min(SLICE_MS) as u64))
    }
}

impl Waits for RealWaits<'_> {
    fn next(&mut self, deadline_ms: i64) {
        while let Some(slice) = self.pause.left(deadline_ms) {
            let Some((_, batches)) = &self.batches else {
                std::thread::sleep(slice);
                continue;
            };
            match batches.recv_timeout(slice) {
                Ok(batch) if batch.rescan || names(&batch, &self.file, self.any_jsonl) => return,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.batches = None,
            }
        }
    }
}

fn names(batch: &Batch, file: &Path, any_jsonl: bool) -> bool {
    batch.paths.iter().any(|p| {
        p.file_name() == file.file_name()
            || (any_jsonl && p.extension().is_some_and(|e| e == "jsonl"))
    })
}

impl Pause for RealPause {
    fn watch(&self, file: &Path, any_jsonl: bool) -> Box<dyn Waits + '_> {
        let folder = file.parent().map(Path::to_path_buf);
        let batches = folder.and_then(|f| Watcher::start(vec![f], WATCH).ok());
        Box::new(RealWaits {
            pause: self,
            file: file.to_path_buf(),
            any_jsonl,
            batches,
        })
    }

    fn sleep(&self, deadline_ms: i64) {
        while let Some(slice) = self.left(deadline_ms) {
            std::thread::sleep(slice);
        }
    }
}

#[derive(Default)]
struct Watch {
    path: Option<String>,
    agent: Option<Agent>,
    message: Option<Message>,
    read: Option<(String, u64, Option<SystemTime>)>,
    seen: Option<Seen>,
    listed: bool,
    process: Option<(u32, Option<i64>)>,
    changed: bool,
    missing_since: Option<i64>,
    gone_since: Option<i64>,
}

pub struct TranscriptVerifier {
    files: Arc<dyn JobFiles>,
    clock: Clock,
    pause: Arc<dyn Pause>,
    safe: Arc<SafeModes>,
    closed: Arc<AtomicBool>,
    watches: Mutex<HashMap<String, Watch>>,
}

impl TranscriptVerifier {
    pub fn new(roots: Roots, safe: Arc<SafeModes>) -> TranscriptVerifier {
        let clock = real_clock();
        let closed = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(RealPause {
            clock: clock.clone(),
            closed: closed.clone(),
        });
        let mut verifier = TranscriptVerifier::with_parts(live_files(roots), clock, pause, safe);
        verifier.closed = closed;
        verifier
    }

    pub(crate) fn with_parts(
        files: Arc<dyn JobFiles>,
        clock: Clock,
        pause: Arc<dyn Pause>,
        safe: Arc<SafeModes>,
    ) -> TranscriptVerifier {
        TranscriptVerifier {
            files,
            clock,
            pause,
            safe,
            closed: Arc::new(AtomicBool::new(false)),
            watches: Mutex::new(HashMap::new()),
        }
    }

    fn transcript(&self, row: &Row) -> Option<(Agent, String)> {
        let agent = self.files.row(&row.session)?;
        let path = self.files.transcript(&agent)?;
        Some((agent, path))
    }

    fn put(&self, id: &str, watch: Watch) {
        lock(&self.watches).insert(id.to_string(), watch);
    }

    #[cfg(test)]
    pub(crate) fn watching(&self) -> usize {
        lock(&self.watches).len()
    }

    fn take(&self, id: &str) -> Option<Watch> {
        lock(&self.watches).remove(id)
    }

    fn refresh(&self, row: &Row, watch: &mut Watch, before: Seen, now_ms: i64) -> bool {
        let agent = self.files.row(&row.session);
        let state = self.files.state(&row.session);
        if let Some(agent) = &agent {
            if let Some(path) = self.files.transcript(agent) {
                watch.path = Some(path);
            }
            watch.agent = Some(agent.clone());
        }
        let missing = agent.is_none() && state.is_none();
        watch.missing_since = match (missing, watch.missing_since) {
            (false, _) => None,
            (true, since) => Some(since.unwrap_or(now_ms)),
        };
        let vanished_at = watch
            .missing_since
            .map(|since| since + GONE_AFTER_MS)
            .filter(|at| now_ms >= *at);
        let stopped = state.as_deref() == Some("stopped");
        match (watch.process, self.files.process(&row.session)) {
            (Some(first), Some(now)) => watch.changed |= first != now,
            (None, now) => watch.process = now,
            (Some(_), None) => {}
        }
        let changed = watch.changed;
        if let (Some(path), Some(agent)) = (watch.path.clone(), watch.agent.clone()) {
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            let folder = Path::new(&path)
                .parent()
                .and_then(|dir| std::fs::metadata(dir).ok())
                .and_then(|meta| meta.modified().ok());
            let read = Some((path.clone(), size, folder));
            if watch.read != read {
                let message = watch.message.get_or_insert_with(|| message_of(row));
                watch.seen =
                    count_anchored(self.files.as_ref(), &agent, &path, row, message, before);
                watch.listed = queue_lists(&path, message).unwrap_or(false);
                watch.read = read;
            }
        }
        let still = watch.listed && !stopped && !changed && vanished_at.is_none();
        if still {
            watch.gone_since = None;
        } else if watch.gone_since.is_none() {
            let only_vanished = watch.listed && !stopped && !changed;
            watch.gone_since = Some(match (only_vanished, vanished_at) {
                (true, Some(at)) => at,
                _ => now_ms,
            });
        }
        still
    }
}

impl Verifier for TranscriptVerifier {
    fn seen(&self, row: &Row) -> Baseline {
        self.take(&row.id);
        let found = self.transcript(row);
        let anchor = Some(Anchor {
            file: found.as_ref().map(|(_, path)| path.clone()),
            slack_ms: SINCE_BEFORE_MS,
        });
        let seen = found
            .and_then(|(agent, path)| {
                let message = message_of(row);
                count_row(
                    self.files.as_ref(),
                    &agent,
                    &path,
                    row,
                    &message,
                    SINCE_BEFORE_MS,
                )
            })
            .unwrap_or_default();
        Baseline { seen, anchor }
    }

    fn wait(&self, row: &Row, before: Seen, entered_ms: i64) -> Verdict {
        let message = message_of(row);
        let any_jsonl = is_clear(&message);
        let mut found = self.transcript(row);
        let mut waits = found
            .as_ref()
            .map(|(_, path)| self.pause.watch(Path::new(path), any_jsonl));
        let mut noted = false;
        loop {
            let now = (self.clock)();
            if let Some(current) = self
                .transcript(row)
                .filter(|(_, path)| found.as_ref().map(|(_, p)| p) != Some(path))
            {
                waits = Some(self.pause.watch(Path::new(&current.1), any_jsonl));
                found = Some(current);
            }
            let seen = found.as_ref().and_then(|(agent, path)| {
                count_anchored(self.files.as_ref(), agent, path, row, &message, before)
            });
            let within = line_within(seen, before);
            if !noted && (within || now - entered_ms >= QUEUE_LINE_MS) {
                self.safe.note(&row.session, within);
                noted = true;
            }
            match verdict(seen, before, now - entered_ms) {
                Verdict::Pending => {}
                Verdict::InQueue => {
                    let watch = Watch {
                        path: found.as_ref().map(|(_, path)| path.clone()),
                        agent: found.as_ref().map(|(agent, _)| agent.clone()),
                        listed: true,
                        process: self.files.process(&row.session),
                        ..Watch::default()
                    };
                    self.put(&row.id, watch);
                    return Verdict::InQueue;
                }
                other => return other,
            }
            if self.closed.load(Ordering::SeqCst) {
                return Verdict::Pending;
            }
            let watching = now < entered_ms + VERIFY_WATCH_MS;
            let deadline = match (noted, watching) {
                (false, _) => entered_ms + QUEUE_LINE_MS,
                (true, true) => entered_ms + VERIFY_WATCH_MS,
                (true, false) => entered_ms + VERIFY_LOOK_MS,
            };
            match (&mut waits, watching) {
                (Some(waits), true) => waits.next(deadline),
                _ => {
                    waits = None;
                    self.pause.sleep(deadline);
                }
            }
        }
    }

    fn watch(&self, row: &Row, before: Seen, now_ms: i64) -> QueuedVerdict {
        let mut watch = self.take(&row.id).unwrap_or_default();
        let still = self.refresh(row, &mut watch, before, now_ms);
        let judged = queued_verdict(watch.seen, before, still, watch.gone_since, now_ms);
        if judged == QueuedVerdict::Stay {
            self.put(&row.id, watch);
        }
        judged
    }

    fn next_look_ms(&self, row: &Row) -> Option<i64> {
        lock(&self.watches)
            .get(&row.id)
            .and_then(|w| w.gone_since)
            .map(|since| since + VERIFY_LOOK_MS)
    }

    fn looks_again(&self) -> bool {
        true
    }

    fn found_late(&self, row: &Row, before: Seen) -> bool {
        let message = message_of(row);
        let seen = self.transcript(row).and_then(|(agent, path)| {
            count_anchored(self.files.as_ref(), &agent, &path, row, &message, before)
        });
        verdict(seen, before, 0) == Verdict::Sent
    }

    fn forget(&self, id: &str) {
        self.take(id);
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "outbox_verify_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "outbox_late_tests.rs"]
mod late_tests;
