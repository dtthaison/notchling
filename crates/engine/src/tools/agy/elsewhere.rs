//! A conversation another `agy` holds: a delegation (`notchling delegate agy`), an `agy` in a
//! terminal, another daemon. Two `agy` processes on one conversation don't see each other's turns:
//! the second one's `session/load` replays only what was written when it loaded, and a prompt sent
//! through it makes Antigravity stop the first one's work as after a server restart. So while
//! another process holds the conversation, the daemon starts none of its own and shows the
//! conversation read-only from Antigravity's own log (`brain/<id>/.system_generated/logs/
//! transcript.jsonl`), which every process appends to.
//!
//! An `agy` that has a conversation loaded keeps its `brain/<id>` folder open, so `lsof` on that
//! folder names the holders; the daemon's own are its children. Found with the commands through
//! [`Procs`], so a test runs none.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::accounts::procs::Procs;
use crate::sessions::iso::parse_iso_ms;
use crate::transcript::model::{ChatItem, Conversation, ItemKind};

const LSOF: &str = "/usr/sbin/lsof";
const PS: &str = "/bin/ps";
const TIMEOUT: Duration = Duration::from_secs(3);
/// The most of the log's end that is read; the newest steps are at its end.
const LOG_TAIL: u64 = 4 << 20;

/// Another `agy` holding a conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    /// Started by a delegation (`notchling delegate`), which ends on its own.
    pub delegation: bool,
}

impl Holder {
    /// What Chat says about it, under the conversation and on the reply box.
    pub fn note(&self) -> &'static str {
        if self.delegation {
            DELEGATION_NOTE
        } else {
            OTHER_NOTE
        }
    }
}

pub const DELEGATION_NOTE: &str =
    "A delegation is running this session. Read only here until it ends.";
pub const OTHER_NOTE: &str =
    "Another Antigravity CLI has this session open. Read only here until it closes.";

/// The conversation's folder in Antigravity's data folder (`root` is `~/.gemini/antigravity-cli`).
pub fn brain_dir(root: &Path, session: &str) -> PathBuf {
    root.join("brain").join(session)
}

/// Its log, which every `agy` on the conversation appends to.
pub fn log_path(root: &Path, session: &str) -> PathBuf {
    brain_dir(root, session).join(".system_generated/logs/transcript.jsonl")
}

/// The `agy` processes in `lsof -F pc` output.
pub fn agy_pids(lsof: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut pid: Option<u32> = None;
    for line in lsof.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.trim().parse().ok();
        } else if let Some(command) = line.strip_prefix('c') {
            if let Some(p) = pid.take().filter(|_| command.trim() == "agy") {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Whether a parent's command line is a delegation's.
pub fn is_delegation(parent_command: &str) -> bool {
    let words: Vec<&str> = parent_command.split_whitespace().collect();
    let named = words.iter().any(|w| {
        w.rsplit('/')
            .next()
            .is_some_and(|n| n.eq_ignore_ascii_case("notchling"))
    });
    named && words.iter().any(|w| *w == "delegate" || *w == "--delegate")
}

/// The first `agy` holding the conversation whose parent isn't `own` (the daemon). `None` when
/// nothing else holds it, or the folder doesn't exist.
pub fn find(procs: &dyn Procs, root: &Path, session: &str, own: u32) -> Option<Holder> {
    let dir = brain_dir(root, session);
    if !dir.is_dir() {
        return None;
    }
    let dir = dir.to_string_lossy().into_owned();
    let out = procs.capture(LSOF, &["-nP", "-F", "pc", "--", &dir], TIMEOUT)?;
    agy_pids(&out).into_iter().find_map(|pid| {
        let p = pid.to_string();
        let parent: u32 = procs
            .capture(PS, &["-o", "ppid=", "-p", &p], TIMEOUT)?
            .trim()
            .parse()
            .ok()?;
        if parent == own {
            return None;
        }
        let command = procs
            .capture(PS, &["-o", "command=", "-p", &parent.to_string()], TIMEOUT)
            .unwrap_or_default();
        Some(Holder {
            pid,
            delegation: is_delegation(&command),
        })
    })
}

/// The log's last change, to tell when there is more to show.
pub fn log_stamp(root: &Path, session: &str) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(log_path(root, session)).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// The conversation as the log has it, read-only: your messages, its replies, and `note` at the
/// end; and whether a turn is still going (your last message has no reply after it).
pub fn from_log(root: &Path, session: &str, note: &str) -> (Conversation, bool) {
    let lines = read_tail(&log_path(root, session));
    let (mut conversation, working) = from_lines(&lines);
    conversation.items.push(ChatItem {
        id: "agy-elsewhere".into(),
        kind: ItemKind::notice(note, "lock"),
        date: None,
    });
    if working {
        conversation.activity = Some("Working elsewhere…".into());
    }
    (conversation, working)
}

fn read_tail(path: &Path) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(LOG_TAIL);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut data = Vec::new();
    if file.read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&data);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    // A cut first line is part of an older step.
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

/// Your messages (`USER_INPUT`, the text inside `<USER_REQUEST>`) and its replies (a
/// `PLANNER_RESPONSE` with text); tool steps and system notes are left out.
pub fn from_lines(lines: &[String]) -> (Conversation, bool) {
    let mut conversation = Conversation::default();
    let mut working = false;
    for (n, line) in lines.iter().enumerate() {
        let Ok(step) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let content = step.get("content").and_then(Value::as_str).unwrap_or("");
        let date = step
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(parse_iso_ms)
            .map(|ms| ms as f64 / 1000.0);
        let kind = match step.get("type").and_then(Value::as_str) {
            Some("USER_INPUT") => {
                working = true;
                ItemKind::user(&user_request(content))
            }
            Some("PLANNER_RESPONSE") if !content.trim().is_empty() => {
                working = false;
                ItemKind::Assistant {
                    text: content.to_owned(),
                }
            }
            _ => continue,
        };
        conversation.items.push(ChatItem {
            id: format!("agy-log-{n}"),
            kind,
            date,
        });
    }
    (conversation, working)
}

/// The text you typed: inside `<USER_REQUEST>`, else all of it.
fn user_request(content: &str) -> String {
    let inner = content
        .split_once("<USER_REQUEST>")
        .and_then(|(_, rest)| rest.split_once("</USER_REQUEST>"))
        .map(|(inner, _)| inner)
        .unwrap_or(content);
    inner.trim().to_owned()
}

#[cfg(test)]
#[path = "elsewhere_tests.rs"]
mod tests;
