//! Antigravity's subagents (Swift `Delegate.AgyProgress`, `agyProgress`, and the watch in
//! `AgyDelegate.awaitSubagents`).
//!
//! agy's image generator (and other subagents) run on after the turn ends: each one leaves a
//! message in the parent's mailbox when it is done, agy wakes the parent with it, and the parent's
//! answer goes only to its transcript (no prompt is open, so ACP sends nothing). Closing agy
//! before then kills the subagent, so the run waits, woken by changes to the session's files.
//! `system` is the session's `brain/<id>/.system_generated`; it is a parameter.

use super::media::safe_component;
use notchling_platform::watch::{WatchConfig, Watcher};
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Where an agy turn stands once `session/prompt` has answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// No subagent started since `since`: the turn's own reply stands.
    Finished,
    /// Subagents still working, or the parent has not answered their messages yet.
    Waiting { subagents: Vec<String> },
    /// Every subagent reported and the parent answered after the last one.
    Answered {
        text: String,
        subagents: Vec<String>,
    },
}

/// The most of the transcript's end that is read (it can be large; the answer is at its end).
const TRANSCRIPT_TAIL: u64 = 4 << 20;
/// A mailbox or subagent file is a few lines; a bigger one is not read.
const SMALL_FILE: u64 = 1 << 20;
/// Files looked at per folder.
const MAX_FILES: usize = 5000;
/// A file counts as this run's when written this long before it began.
const SLACK: Duration = Duration::from_secs(1);

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .take(MAX_FILES)
        .collect()
}

fn read_small(path: &Path) -> Option<Value> {
    let file = fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(SMALL_FILE).read_to_end(&mut data).ok()?;
    serde_json::from_slice(&data).ok()
}

/// The lines in the last `TRANSCRIPT_TAIL` bytes of the transcript that are JSON objects.
fn transcript_lines(path: &Path) -> Vec<Value> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(TRANSCRIPT_TAIL);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut data = Vec::new();
    if file.take(TRANSCRIPT_TAIL).read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&data);
    let mut lines = text.lines();
    if start > 0 {
        // The first line may be cut in two.
        lines.next();
    }
    lines
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(Value::is_object)
        .collect()
}

pub fn progress(system: &Path, since: SystemTime) -> Progress {
    let floor = since.checked_sub(SLACK).unwrap_or(since);
    let spawned: Vec<String> = json_files(&system.join("subagents"))
        .into_iter()
        .filter(|p| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= floor)
        })
        .filter_map(|p| {
            read_small(&p)?
                .get("conversationId")?
                .as_str()
                .map(String::from)
        })
        .collect();
    if spawned.is_empty() {
        return Progress::Finished;
    }
    let reported: Vec<String> = json_files(&system.join("messages"))
        .into_iter()
        .filter_map(|p| read_small(&p)?.get("sender")?.as_str().map(String::from))
        .collect();
    if !spawned.iter().all(|s| reported.contains(s)) {
        return Progress::Waiting { subagents: spawned };
    }
    // The parent's last answer must come after the system message that woke it.
    let lines = transcript_lines(&system.join("logs").join("transcript.jsonl"));
    let kind = |l: &Value| l.get("type").and_then(Value::as_str).map(String::from);
    let answer = lines
        .iter()
        .rposition(|l| kind(l).as_deref() == Some("SYSTEM_MESSAGE"))
        .filter(|wake| lines.len() - 1 > *wake)
        .and_then(|_| lines.last())
        .filter(|last| {
            kind(last).as_deref() == Some("PLANNER_RESPONSE")
                && last.get("status").and_then(Value::as_str) == Some("DONE")
        })
        .and_then(|last| last.get("content")?.as_str())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    match answer {
        Some(text) => Progress::Answered {
            text,
            subagents: spawned,
        },
        None => Progress::Waiting { subagents: spawned },
    }
}

/// How a wait ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Waited {
    /// Nothing more to wait for: `text` is the parent's answer to its subagents, when there were
    /// any; `subagents` are the ids that made files of their own.
    Done {
        text: Option<String>,
        subagents: Vec<String>,
    },
    /// `stop` said so first.
    Stopped { subagents: Vec<String> },
    /// The session's folder can't be watched.
    CannotWatch,
}

/// A recheck at least this often even when nothing is heard, so a missed event is a delay and not
/// a hang.
const RECHECK: Duration = Duration::from_secs(2);
/// How often `stop` is asked.
const TICK: Duration = Duration::from_millis(100);

/// Waits until every subagent started since `since` has reported and the parent has answered.
/// `announce` is told once, with how many subagents are working, before the wait begins.
pub fn wait(
    system: &Path,
    since: SystemTime,
    stop: &dyn Fn() -> bool,
    announce: &mut dyn FnMut(usize),
) -> Waited {
    let mut subagents = match progress(system, since) {
        Progress::Finished => {
            return Waited::Done {
                text: None,
                subagents: Vec::new(),
            }
        }
        Progress::Answered { text, subagents } => {
            return Waited::Done {
                text: Some(text),
                subagents,
            }
        }
        Progress::Waiting { subagents } => subagents,
    };
    announce(subagents.len());
    let config = WatchConfig {
        poll: Duration::from_millis(250),
        debounce: Duration::from_millis(250),
        max_latency: Duration::from_secs(1),
    };
    let Ok((_watcher, events)) = Watcher::start(vec![system.to_path_buf()], config) else {
        return Waited::CannotWatch;
    };
    let mut checked = Instant::now();
    loop {
        if stop() {
            return Waited::Stopped { subagents };
        }
        let heard = match events.recv_timeout(TICK) {
            Ok(_) => true,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Waited::CannotWatch,
        };
        if !heard && checked.elapsed() < RECHECK {
            continue;
        }
        checked = Instant::now();
        match progress(system, since) {
            Progress::Finished => {
                return Waited::Done {
                    text: None,
                    subagents,
                }
            }
            Progress::Answered { text, subagents } => {
                return Waited::Done {
                    text: Some(text),
                    subagents,
                }
            }
            Progress::Waiting { subagents: now } => subagents = now,
        }
    }
}

/// The subagent ids worth looking at for files of their own (plain folder names only).
pub fn media_sessions(session: &str, subagents: &[String]) -> Vec<String> {
    std::iter::once(session.to_string())
        .chain(subagents.iter().cloned())
        .filter(|s| safe_component(s))
        .collect()
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
