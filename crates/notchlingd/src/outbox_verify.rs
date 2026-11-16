use crate::outbox_courier::{live_files, real_clock, Clock, JobFiles};
use crate::outbox_postman::Verifier;
use notchling_engine::outbox::{
    line_within, queued_verdict, verdict, QueuedVerdict, Row, Verdict, GONE_AFTER_MS,
    QUEUE_LINE_MS, VERIFY_LOOK_MS, VERIFY_WATCH_MS,
};
use notchling_engine::sessions::Roots;
use notchling_engine::transcript::locate::successor;
use notchling_engine::transcript::message::Message;
use notchling_engine::transcript::queue_state::{queue_state, QueueState};
use notchling_engine::transcript::sent::{count_since, Seen};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

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

fn message_of(row: &Row) -> Message {
    Message::parse(&row.text, Path::is_file)
}

fn is_clear(message: &Message) -> bool {
    message
        .command
        .as_ref()
        .is_some_and(|(name, _)| name == "clear")
}

fn made_ms(path: &str) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let at = meta.created().or_else(|_| meta.modified()).ok()?;
    let ms = at.duration_since(UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(ms).ok()
}

fn cleared_after(path: &str, name: &str, since_ms: i64) -> bool {
    successor(path, name)
        .and_then(|next| made_ms(&next))
        .is_some_and(|made| made >= since_ms)
}

pub(crate) fn count_row(path: &str, name: &str, row: &Row) -> Option<Seen> {
    let since = row.attempt_ms? - SINCE_BEFORE_MS;
    let message = message_of(row);
    let mut seen = count_since(path, &message, since);
    if is_clear(&message) && cleared_after(path, name, since) {
        seen.get_or_insert_with(Seen::default).taken += 1;
    }
    seen
}

#[derive(Default)]
struct Watch {
    path: Option<String>,
    name: String,
    read: Option<(String, u64)>,
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

    fn transcript(&self, row: &Row) -> Option<(String, String)> {
        let agent = self.files.row(&row.session)?;
        Some((agent.transcript_path?, agent.name))
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

    fn refresh(&self, row: &Row, watch: &mut Watch, now_ms: i64) -> bool {
        let agent = self.files.row(&row.session);
        let state = self.files.state(&row.session);
        if let Some(agent) = &agent {
            if agent.transcript_path.is_some() {
                watch.path = agent.transcript_path.clone();
            }
            watch.name = agent.name.clone();
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
        if let Some(path) = watch.path.clone() {
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            let read = Some((path.clone(), size));
            if watch.read != read {
                watch.seen = count_row(&path, &watch.name, row);
                watch.listed = queue_state(&path).is_some_and(|q| q.listed(&message_of(row)) > 0);
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
    fn seen(&self, row: &Row) -> Option<Seen> {
        self.take(&row.id);
        let (path, name) = self.transcript(row)?;
        count_row(&path, &name, row)
    }

    fn wait(&self, row: &Row, before: Seen, entered_ms: i64) -> Verdict {
        let found = self.transcript(row);
        let mut waits = found.as_ref().map(|(path, _)| {
            self.pause
                .watch(Path::new(path), is_clear(&message_of(row)))
        });
        let mut noted = false;
        loop {
            let now = (self.clock)();
            let seen = found
                .as_ref()
                .and_then(|(path, name)| count_row(path, name, row));
            let within = line_within(seen, before);
            if !noted && (within || now - entered_ms >= QUEUE_LINE_MS) {
                self.safe.note(&row.session, within);
                noted = true;
            }
            match verdict(seen, before, now - entered_ms) {
                Verdict::Pending => {}
                Verdict::InQueue => {
                    let watch = Watch {
                        path: found.as_ref().map(|(path, _)| path.clone()),
                        name: found
                            .as_ref()
                            .map(|(_, name)| name.clone())
                            .unwrap_or_default(),
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
        let still = self.refresh(row, &mut watch, now_ms);
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
