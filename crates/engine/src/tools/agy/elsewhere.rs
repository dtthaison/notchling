use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::accounts::procs::Procs;
use crate::sessions::iso::parse_iso_ms;
use crate::transcript::chat::cap_output;
use crate::transcript::glance::{self, has, TailParser};
use crate::transcript::model::{ChatItem, Conversation, ItemKind, Timestamp, ToolCall, ToolKind};
use crate::transcript::text::{first_line, last_path_component, prefix_chars};
use crate::transcript::util::json_sorted;

const LSOF: &str = "/usr/sbin/lsof";
const PS: &str = "/bin/ps";
const TIMEOUT: Duration = Duration::from_secs(3);
const LOG_TAIL: u64 = 4 << 20;

pub use crate::transcript::glance::Peeked;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub delegation: bool,
}

impl Holder {
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

pub fn brain_dir(root: &Path, session: &str) -> PathBuf {
    root.join("brain").join(session)
}

pub fn log_path(root: &Path, session: &str) -> PathBuf {
    brain_dir(root, session).join(".system_generated/logs/transcript.jsonl")
}

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

pub fn is_delegation(parent_command: &str) -> bool {
    let words: Vec<&str> = parent_command.split_whitespace().collect();
    let named = words.iter().any(|w| {
        w.rsplit('/')
            .next()
            .is_some_and(|n| n.eq_ignore_ascii_case("notchling"))
    });
    named && words.iter().any(|w| *w == "delegate" || *w == "--delegate")
}

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

pub fn log_stamp(root: &Path, session: &str) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(log_path(root, session)).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

pub fn from_log(root: &Path, session: &str, note: &str) -> (Conversation, bool) {
    from_log_file(&log_path(root, session), Some(note))
}

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

fn planner_tokens(line: &[u8]) -> Option<u64> {
    if !has(line, b"PLANNER_RESPONSE") {
        return None;
    }
    let step: Value = serde_json::from_slice(line).ok()?;
    if step.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE") {
        return None;
    }
    let n = |key: &str| step.get(key).and_then(Value::as_u64).unwrap_or(0);
    let total = n("input_tokens")
        .saturating_add(n("cache_read_tokens"))
        .saturating_add(n("output_tokens"));
    (total > 0).then_some(total)
}

fn recent_steps(lines: &[&[u8]]) -> Conversation {
    let lines: Vec<String> = lines
        .iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();
    from_lines(&lines).0
}

pub fn peek(path: &Path, with_recent: bool) -> Option<Peeked> {
    glance::peek(
        &path.to_string_lossy(),
        planner_tokens,
        with_recent.then_some(recent_steps as TailParser),
    )
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
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

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
    waiting: VecDeque<(usize, usize)>,
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

fn result_text(content: &str) -> Option<String> {
    let body: Vec<&str> = content
        .lines()
        .skip_while(|l| l.starts_with("Created At:") || l.starts_with("Completed At:"))
        .collect();
    let text = cap_output(&body.join("\n"));
    (!text.is_empty()).then_some(text)
}

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

#[cfg(test)]
#[path = "elsewhere_peek_tests.rs"]
mod peek_tests;
