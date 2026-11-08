//! What `sd.py worker status --json` prints (Swift `WorkerStatus`; sd 1.19.1, `Worker.status()` in
//! sdlib/worker.py is the contract). A machine never sends this to us: the poller reads it over ssh.
//!
//! It decodes leniently on purpose, because the worker on a machine can be older or newer than
//! Notchling: an unknown key is ignored, and a missing key, a `null` or a value of the wrong type
//! becomes the field's default (`None`, 0, "" or empty). One malformed entry in `ready`,
//! `needs_you` or `stuck` is dropped and the rest are kept. [`WorkerStatus::decode`] reads
//! snake_case keys and ISO-8601 dates with or without fractional seconds (as seconds since 1970).
//!
//! Everything in here came from a machine: it is data to show, and the actions treat each field as
//! untrusted before it reaches a command.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::transcript::util::parse_date;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkerStatus {
    pub machine: String,
    pub version: String,
    /// Why the worker is paused (host load or the production health check), `None` while it works.
    pub paused: Option<String>,
    /// `github`, `claude`, `host` -> "ok" or "error: ..." / "limit" / "paused: ...".
    pub health: BTreeMap<String, String>,
    pub current: Option<Job>,
    pub ready: Vec<ReadyEntry>,
    pub needs_you: Vec<NeedsYouEntry>,
    pub stuck: Vec<StuckEntry>,
    pub jobs_tonight: i64,
    pub weekly_pct: Option<f64>,
    pub token_expires_in_days: Option<i64>,
    /// When the worker process last started (a reboot or a restart); `None` before its first start.
    pub booted_at: Option<f64>,
    /// When the worker's state file was first made.
    pub since: Option<f64>,
    /// GitHub writes the worker gave up on. `status` itself does not print them in 1.19.1 (only
    /// `health.github` says "N outbox items failed"), so this is 0 unless a worker sends a list
    /// (counted) or a number.
    pub outbox_failed: i64,
    /// Issues queued for a kind no machine serves, e.g. `["ios"]`.
    pub unrouted: Vec<String>,
    /// `None` when the machine has no TestFlight poller.
    pub testflight: Option<TestFlightStatus>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub repo: String,
    pub issue: i64,
    pub branch: Option<String>,
    pub stage: String,
    pub started_at: Option<f64>,
}

/// The TestFlight feedback poller's state (`testflight.status()` in sdlib/testflight.py).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestFlightStatus {
    /// No new feedback for over 7 days.
    pub quiet: bool,
    pub last_new_at: Option<f64>,
    /// Submissions skipped after GitHub refused them.
    pub failed: Option<TestFlightFailed>,
    /// The poller's state file is damaged.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestFlightFailed {
    pub count: i64,
    pub newest_id: String,
    pub error: String,
    pub at: Option<f64>,
}

/// A branch the worker verified and left ready to merge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadyEntry {
    pub repo: String,
    pub issue: i64,
    pub branch: String,
    pub commit: String,
    /// The worktree on the machine.
    pub worktree: String,
    /// The verify evidence on the machine; empty from a worker that does not send it.
    pub evidence_dir: String,
    pub ready_at: Option<f64>,
}

/// A job that waits for you: a question, a failed verify, or a triage decision.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeedsYouEntry {
    pub repo: String,
    pub issue: i64,
    /// `None` for an issue the worker's triage sent to you before any branch existed.
    pub branch: Option<String>,
    /// `None` for the same triage entries.
    pub worktree: Option<String>,
    pub reason: String,
    pub question: Option<String>,
    pub recommend: Option<String>,
    pub options: Vec<String>,
}

/// A job the worker gave up on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StuckEntry {
    pub repo: String,
    pub issue: i64,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub reason: String,
    pub report_tail: String,
}

/// A date as ISO 8601, with or without fractional seconds (the worker writes
/// `2026-11-05T22:10:31Z`). Seconds since 1970.
pub fn parse_status_date(text: &str) -> Option<f64> {
    parse_date(text)
}

// Lenient readers: the value for a key, or `None` when it is missing, null or of another type.

pub(super) fn string(o: &Map<String, Value>, key: &str) -> Option<String> {
    o.get(key)?.as_str().map(str::to_string)
}

pub(super) fn int(o: &Map<String, Value>, key: &str) -> Option<i64> {
    let v = o.get(key)?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    // 3.0 is an integer to Foundation's decoder; 3.5 is not.
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f.abs() < 9.0e15).then_some(f as i64)
}

fn number(o: &Map<String, Value>, key: &str) -> Option<f64> {
    o.get(key)?.as_f64()
}

fn boolean(o: &Map<String, Value>, key: &str) -> Option<bool> {
    o.get(key)?.as_bool()
}

pub(super) fn date(o: &Map<String, Value>, key: &str) -> Option<f64> {
    parse_status_date(o.get(key)?.as_str()?)
}

fn object<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    o.get(key)?.as_object()
}

/// An array of strings; one element of another type makes the whole value `None`.
fn strings(o: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    o.get(key)?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// The elements that are objects and decode; anything else is dropped.
fn lossy<T>(o: &Map<String, Value>, key: &str, read: fn(&Map<String, Value>) -> T) -> Vec<T> {
    let Some(Value::Array(all)) = o.get(key) else {
        return Vec::new();
    };
    all.iter().filter_map(Value::as_object).map(read).collect()
}

impl WorkerStatus {
    /// Reads one `worker status --json` document; `None` when it is not a JSON object at all.
    pub fn decode(data: &[u8]) -> Option<WorkerStatus> {
        let Value::Object(o) = serde_json::from_slice::<Value>(data).ok()? else {
            return None;
        };
        Some(WorkerStatus::read(&o))
    }

    fn read(o: &Map<String, Value>) -> WorkerStatus {
        // key by key: one value that is not a string drops that key only
        let health = object(o, "health")
            .map(|h| {
                h.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let outbox_failed =
            int(o, "outbox_failed").unwrap_or_else(|| match o.get("outbox_failed") {
                Some(Value::Array(list)) => list.len() as i64,
                _ => 0,
            });
        WorkerStatus {
            machine: string(o, "machine").unwrap_or_default(),
            version: string(o, "version").unwrap_or_default(),
            paused: string(o, "paused"),
            health,
            current: object(o, "current").map(Job::read),
            ready: lossy(o, "ready", ReadyEntry::read),
            needs_you: lossy(o, "needs_you", NeedsYouEntry::read),
            stuck: lossy(o, "stuck", StuckEntry::read),
            jobs_tonight: int(o, "jobs_tonight").unwrap_or(0),
            weekly_pct: number(o, "weekly_pct"),
            token_expires_in_days: int(o, "token_expires_in_days"),
            booted_at: date(o, "booted_at"),
            since: date(o, "since"),
            outbox_failed,
            unrouted: strings(o, "unrouted").unwrap_or_default(),
            testflight: object(o, "testflight").map(TestFlightStatus::read),
        }
    }
}

impl Job {
    fn read(o: &Map<String, Value>) -> Job {
        Job {
            repo: string(o, "repo").unwrap_or_default(),
            issue: int(o, "issue").unwrap_or(0),
            branch: string(o, "branch"),
            stage: string(o, "stage").unwrap_or_default(),
            started_at: date(o, "started_at"),
        }
    }
}

impl TestFlightStatus {
    fn read(o: &Map<String, Value>) -> TestFlightStatus {
        TestFlightStatus {
            quiet: boolean(o, "quiet").unwrap_or(false),
            last_new_at: date(o, "last_new_at"),
            failed: object(o, "failed").map(|f| TestFlightFailed {
                count: int(f, "count").unwrap_or(0),
                newest_id: string(f, "newest_id").unwrap_or_default(),
                error: string(f, "error").unwrap_or_default(),
                at: date(f, "at"),
            }),
            error: string(o, "error"),
        }
    }
}

impl ReadyEntry {
    fn read(o: &Map<String, Value>) -> ReadyEntry {
        ReadyEntry {
            repo: string(o, "repo").unwrap_or_default(),
            issue: int(o, "issue").unwrap_or(0),
            branch: string(o, "branch").unwrap_or_default(),
            commit: string(o, "commit").unwrap_or_default(),
            worktree: string(o, "worktree").unwrap_or_default(),
            evidence_dir: string(o, "evidence_dir").unwrap_or_default(),
            ready_at: date(o, "ready_at"),
        }
    }
}

impl NeedsYouEntry {
    fn read(o: &Map<String, Value>) -> NeedsYouEntry {
        NeedsYouEntry {
            repo: string(o, "repo").unwrap_or_default(),
            issue: int(o, "issue").unwrap_or(0),
            branch: string(o, "branch"),
            worktree: string(o, "worktree"),
            reason: string(o, "reason").unwrap_or_default(),
            question: string(o, "question"),
            recommend: string(o, "recommend"),
            options: strings(o, "options").unwrap_or_default(),
        }
    }
}

impl StuckEntry {
    fn read(o: &Map<String, Value>) -> StuckEntry {
        StuckEntry {
            repo: string(o, "repo").unwrap_or_default(),
            issue: int(o, "issue").unwrap_or(0),
            branch: string(o, "branch"),
            worktree: string(o, "worktree"),
            reason: string(o, "reason").unwrap_or_default(),
            report_tail: string(o, "report_tail").unwrap_or_default(),
        }
    }
}
