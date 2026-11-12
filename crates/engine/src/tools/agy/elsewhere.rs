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

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::accounts::procs::Procs;
use crate::sessions::iso::parse_iso_ms;
use crate::transcript::chat::cap_output;
use crate::transcript::model::{ChatItem, Conversation, ItemKind, Timestamp, ToolCall, ToolKind};
use crate::transcript::text::{first_line, last_path_component, prefix_chars};
use crate::transcript::util::json_sorted;

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

/// The conversation as the log has it, read-only: your messages, its thinking, replies and tool
/// steps, and `note` at the end; and whether a turn is still going. While one is, `activity`
/// names the call still waiting for its result.
pub fn from_log(root: &Path, session: &str, note: &str) -> (Conversation, bool) {
    from_log_file(&log_path(root, session), Some(note))
}

/// [`from_log`] for a log at `path`, with no note at the end when `note` is `None` (a delegation
/// shown as a subagent has nothing to say about who holds it).
pub fn from_log_file(path: &Path, note: Option<&str>) -> (Conversation, bool) {
    let lines = read_tail(path);
    let (mut conversation, working) = from_lines(&lines);
    if working {
        let open = match conversation.items.last().map(|i| &i.kind) {
            Some(ItemKind::Tools(calls)) => calls.iter().find(|c| !c.finished),
            _ => None,
        };
        conversation.activity = Some(
            open.map(|c| c.title.clone())
                .unwrap_or_else(|| "Working elsewhere…".into()),
        );
    }
    if let Some(note) = note {
        conversation.items.push(ChatItem {
            id: "agy-elsewhere".into(),
            kind: ItemKind::notice(note, "lock"),
            date: None,
        });
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

/// The log's steps as the conversation ACP's replay makes of them, and whether a turn is still
/// going. `USER_INPUT` is your message (the text inside `<USER_REQUEST>`); a `PLANNER_RESPONSE` is
/// its thinking, its reply text and a group of tool calls; each `GENERIC` after it is the result of
/// the next call. System notes, checkpoints and errors of the service itself are left out.
///
/// A turn goes on from your message until a planner step that calls nothing. Item ids come from the
/// line's place in `lines`, not its `step_index`, which restarts when a session does.
pub fn from_lines(lines: &[String]) -> (Conversation, bool) {
    let mut log = Log::default();
    for (n, line) in lines.iter().enumerate() {
        if let Ok(step) = serde_json::from_str::<Value>(line) {
            log.step(n, &step);
        }
    }
    log.finish()
}

#[derive(Default)]
struct Log {
    conversation: Conversation,
    working: bool,
    turn_start: Option<Timestamp>,
    turn_tokens: u64,
    /// Calls with no result step yet, oldest first: the item's index and the call's in it.
    waiting: VecDeque<(usize, usize)>,
    /// Calls whose step says it is still running (a command left in the background).
    running: Vec<(usize, usize)>,
}

impl Log {
    fn step(&mut self, n: usize, step: &Value) {
        let content = step.get("content").and_then(Value::as_str).unwrap_or("");
        let date = step
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(parse_iso_ms)
            .map(|ms| ms as f64 / 1000.0);
        match step.get("type").and_then(Value::as_str) {
            Some("USER_INPUT") => {
                self.close_calls();
                self.working = true;
                self.turn_start = date;
                self.turn_tokens = 0;
                let kind = ItemKind::user(&user_request(content));
                self.push(format!("agy-log-{n}"), kind, date);
            }
            Some("PLANNER_RESPONSE") => self.planner(n, step, content, date),
            Some("GENERIC") => self.result(step, content),
            _ => {}
        }
    }

    fn push(&mut self, id: String, kind: ItemKind, date: Option<Timestamp>) {
        self.conversation.items.push(ChatItem { id, kind, date });
    }

    /// The model went on, so a call it never got a step for is over too.
    fn close_calls(&mut self) {
        let open: Vec<(usize, usize)> = self
            .waiting
            .drain(..)
            .chain(self.running.drain(..))
            .collect();
        for (i, j) in open {
            if let Some(call) = self.call_mut(i, j) {
                call.finished = true;
            }
        }
    }

    fn call_mut(&mut self, item: usize, call: usize) -> Option<&mut ToolCall> {
        match &mut self.conversation.items.get_mut(item)?.kind {
            ItemKind::Tools(calls) => calls.get_mut(call),
            _ => None,
        }
    }

    fn planner(&mut self, n: usize, step: &Value, content: &str, date: Option<Timestamp>) {
        self.close_calls();
        let calls: &[Value] = step
            .get("tool_calls")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        if calls.is_empty() {
            self.working = false;
        } else if !self.working {
            // Woken with no message of yours (a subagent reported): a turn of its own.
            self.working = true;
            self.turn_start = date;
            self.turn_tokens = 0;
        }
        self.turn_tokens += step
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let thinking = step
            .get("thinking")
            .and_then(Value::as_str)
            .map_or("", str::trim);
        if !thinking.is_empty() {
            let kind = ItemKind::Thinking {
                seconds: 0.0,
                text: thinking.to_owned(),
            };
            self.push(format!("agy-log-{n}-thinking"), kind, date);
        }
        if !content.trim().is_empty() {
            let kind = ItemKind::Assistant {
                text: content.to_owned(),
            };
            self.push(format!("agy-log-{n}"), kind, date);
        }
        if calls.is_empty() {
            return;
        }
        let made = calls.iter().enumerate().map(|(k, call)| {
            let args = call.get("args").unwrap_or(&Value::Null);
            let name = call.get("name").and_then(Value::as_str).unwrap_or("tool");
            make_call(&format!("agy-log-{n}-call-{k}"), name, args)
        });
        // Calls with nothing between them are one group, as in ACP's replay.
        if !matches!(
            self.conversation.items.last().map(|i| &i.kind),
            Some(ItemKind::Tools(_))
        ) {
            self.push(
                format!("agy-log-{n}-tools"),
                ItemKind::Tools(Vec::new()),
                date,
            );
        }
        let item = self.conversation.items.len() - 1;
        if let Some(ChatItem {
            kind: ItemKind::Tools(group),
            ..
        }) = self.conversation.items.last_mut()
        {
            for call in made {
                self.waiting.push_back((item, group.len()));
                group.push(call);
            }
        }
    }

    /// A `GENERIC` step is the result of the oldest call without one.
    fn result(&mut self, step: &Value, content: &str) {
        let Some((i, j)) = self.waiting.pop_front() else {
            return;
        };
        let status = step.get("status").and_then(Value::as_str);
        let body = result_text(content);
        let error = step
            .get("error")
            .and_then(Value::as_str)
            .map(cap_output)
            .filter(|e| !e.is_empty());
        let Some(call) = self.call_mut(i, j) else {
            return;
        };
        match status {
            Some("ERROR") => {
                call.finished = true;
                call.is_error = true;
                call.output = error.or(body);
            }
            Some("DONE") | None => {
                call.finished = true;
                call.output = body;
            }
            // Still running: what it printed so far, and no result yet.
            Some(_) => {
                call.output = body;
                self.running.push((i, j));
            }
        }
    }

    fn finish(mut self) -> (Conversation, bool) {
        if self.working {
            self.conversation.turn_start = self.turn_start;
            self.conversation.turn_tokens = self.turn_tokens;
        }
        (self.conversation, self.working)
    }
}

/// A tool step's output: its `content` without the Created At / Completed At lines that head it.
fn result_text(content: &str) -> Option<String> {
    let body: Vec<&str> = content
        .lines()
        .skip_while(|l| l.starts_with("Created At:") || l.starts_with("Completed At:"))
        .collect();
    let text = cap_output(&body.join("\n"));
    (!text.is_empty()).then_some(text)
}

/// A tool argument as text. The log keeps each value as JSON text, a string inside a string
/// (`"\"ls -la\""`), so one that parses as a JSON string is unwrapped; any other text stays.
fn arg(args: &Value, key: &str) -> String {
    match args.get(key) {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(Value::String(inner)) => inner,
            _ => text.clone(),
        },
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// One of Antigravity's tool calls as Chat's step, titled the way Claude Code's are ("Read a.rs",
/// "Ran ls"); a tool not named here shows under its own name.
fn make_call(id: &str, name: &str, args: &Value) -> ToolCall {
    let call =
        |kind: ToolKind, title: String, input: String| ToolCall::new(id, name, kind, title, input);
    let file = |key: &str| {
        let path = arg(args, key);
        (last_path_component(&path).to_owned(), path)
    };
    match name {
        "read_file" => {
            let (file, path) = file("Path");
            call(ToolKind::Read, format!("Read {file}"), path)
        }
        "edit_file" => {
            let (file, path) = file("Path");
            call(ToolKind::Edit, format!("Edited {file}"), path)
        }
        "write_file" => {
            let (file, path) = file("Path");
            call(ToolKind::Write, format!("Wrote {file}"), path)
        }
        "run_command" => {
            let command = arg(args, "CommandLine");
            let summary = arg(args, "toolSummary");
            let title = if summary.trim().is_empty() {
                format!("Ran {}", prefix_chars(first_line(&command), 60))
            } else {
                summary
            };
            call(ToolKind::Shell, title, command)
        }
        "search_web" => {
            let query = arg(args, "query");
            call(
                ToolKind::Web,
                format!("Searched the web for \u{201c}{query}\u{201d}"),
                query,
            )
        }
        _ => {
            let input = match args {
                Value::Object(map) if !map.is_empty() => {
                    json_sorted(args, true).chars().take(400).collect()
                }
                _ => String::new(),
            };
            call(ToolKind::Other, name.to_owned(), input)
        }
    }
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
