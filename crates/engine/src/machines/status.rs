use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::transcript::util::parse_date;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkerStatus {
    pub machine: String,
    pub version: String,
    pub paused: Option<String>,
    pub health: BTreeMap<String, String>,
    pub current: Option<Job>,
    pub ready: Vec<ReadyEntry>,
    pub needs_you: Vec<NeedsYouEntry>,
    pub stuck: Vec<StuckEntry>,
    pub jobs_tonight: i64,
    pub weekly_pct: Option<f64>,
    pub token_expires_in_days: Option<i64>,
    pub booted_at: Option<f64>,
    pub since: Option<f64>,
    pub outbox_failed: i64,
    pub unrouted: Vec<String>,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestFlightStatus {
    pub quiet: bool,
    pub last_new_at: Option<f64>,
    pub failed: Option<TestFlightFailed>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestFlightFailed {
    pub count: i64,
    pub newest_id: String,
    pub error: String,
    pub at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadyEntry {
    pub repo: String,
    pub issue: i64,
    pub branch: String,
    pub commit: String,
    pub worktree: String,
    pub evidence_dir: String,
    pub ready_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeedsYouEntry {
    pub repo: String,
    pub issue: i64,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub reason: String,
    pub question: Option<String>,
    pub recommend: Option<String>,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StuckEntry {
    pub repo: String,
    pub issue: i64,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub reason: String,
    pub report_tail: String,
}

pub fn parse_status_date(text: &str) -> Option<f64> {
    parse_date(text)
}

pub(super) fn string(o: &Map<String, Value>, key: &str) -> Option<String> {
    o.get(key)?.as_str().map(str::to_string)
}

pub(super) fn int(o: &Map<String, Value>, key: &str) -> Option<i64> {
    let v = o.get(key)?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
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

fn strings(o: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    o.get(key)?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn lossy<T>(o: &Map<String, Value>, key: &str, read: fn(&Map<String, Value>) -> T) -> Vec<T> {
    let Some(Value::Array(all)) = o.get(key) else {
        return Vec::new();
    };
    all.iter().filter_map(Value::as_object).map(read).collect()
}

impl WorkerStatus {
    pub fn decode(data: &[u8]) -> Option<WorkerStatus> {
        let Value::Object(o) = serde_json::from_slice::<Value>(data).ok()? else {
            return None;
        };
        Some(WorkerStatus::read(&o))
    }

    fn read(o: &Map<String, Value>) -> WorkerStatus {
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
