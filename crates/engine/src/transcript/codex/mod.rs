mod calls;
mod glance;
mod stamped;

pub use calls::{call, diff, exit_code, output_text, patch_files};
pub use glance::{context_tokens, recent};

use super::model::*;
use super::tail::{Cursors, LineParser};
use super::text::prefix_chars;
use super::util::{count_of, date_field, now_secs, parse_object, str_of};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use stamped::{Stamp, Stamped};

#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
    pub id: String,
    pub cwd: String,
    pub title: Option<String>,
}

pub fn user_text(payload: &Value) -> Option<String> {
    let parts: Vec<&str> = payload
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter(|p| str_of(p, "type") == Some("input_text"))
                .filter_map(|p| str_of(p, "text"))
                .collect()
        })
        .unwrap_or_default();
    let text = parts.join("\n").trim().to_string();
    if text.is_empty() || text.starts_with('<') || text.starts_with("# AGENTS.md") {
        None
    } else {
        Some(text)
    }
}

pub fn title(text: &str) -> String {
    let line = text
        .split(['\n', '\r'])
        .find(|l| !l.is_empty())
        .unwrap_or(text);
    if line.chars().count() > 80 {
        format!("{}…", prefix_chars(line, 79))
    } else {
        line.to_string()
    }
}

fn meta_cache() -> &'static Mutex<HashMap<String, Meta>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Meta>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn meta(path: &str) -> Option<Meta> {
    if let Some(hit) = meta_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
    {
        return Some(hit.clone());
    }
    static UNTITLED: Stamped<Option<Meta>> = Stamped::new();
    match Stamp::of(Path::new(path)) {
        Some(stamp) => UNTITLED.get(path, stamp, || read_meta(path)),
        None => read_meta(path),
    }
}

fn read_meta(path: &str) -> Option<Meta> {
    let mut head = Vec::new();
    File::open(path)
        .ok()?
        .take(3_000_000)
        .read_to_end(&mut head)
        .ok()?;
    let mut found: Option<Meta> = None;
    for line in head.split(|&b| b == b'\n') {
        let Some(map) = parse_object(line) else {
            continue;
        };
        let obj = Value::Object(map);
        let Some(payload) = obj.get("payload").filter(|p| p.is_object()) else {
            continue;
        };
        let kind = str_of(&obj, "type");
        if kind == Some("session_meta") {
            if let Some(id) = str_of(payload, "id") {
                found = Some(Meta {
                    id: id.to_string(),
                    cwd: str_of(payload, "cwd").unwrap_or("").to_string(),
                    title: None,
                });
            }
        } else if let Some(m) = found.as_mut() {
            if kind == Some("response_item")
                && str_of(payload, "type") == Some("message")
                && str_of(payload, "role") == Some("user")
            {
                if let Some(text) = user_text(payload) {
                    m.title = Some(title(&text));
                    break;
                }
            }
        }
    }
    let found = found?;
    if found.title.is_some() {
        meta_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_string(), found.clone());
    }
    Some(found)
}

pub fn turn_open(path: &str) -> bool {
    static OPEN: Stamped<bool> = Stamped::new();
    match Stamp::of(Path::new(path)) {
        Some(stamp) => OPEN.get(path, stamp, || read_turn_open(path)),
        None => false,
    }
}

fn read_turn_open(path: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(size) = file.seek(SeekFrom::End(0)) else {
        return false;
    };
    if file
        .seek(SeekFrom::Start(size.saturating_sub(262_144)))
        .is_err()
    {
        return false;
    }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() {
        return false;
    }
    let last = |word: &str| -> Option<usize> {
        let needle = format!("\"type\":\"{word}\"");
        let n = needle.as_bytes();
        tail.windows(n.len()).rposition(|w| w == n)
    };
    let ended = last("task_complete").max(last("turn_aborted"));
    match last("task_started") {
        Some(started) => ended.is_none_or(|e| started > e),
        None => false,
    }
}

#[derive(Default)]
pub struct Parser {
    items: Vec<ChatItem>,
    facts: SessionFacts,
    turn_start: Option<Timestamp>,
    model: Option<String>,
    open_calls: HashMap<String, (usize, usize)>,
    files: BTreeSet<String>,
    fallback_ids: u64,
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    pub fn consume(&mut self, line: &[u8]) {
        LineParser::consume(self, line);
    }

    pub fn conversation(&self) -> Conversation {
        let mut c = Conversation {
            items: self.items.clone(),
            facts: self.facts.clone(),
            ..Conversation::default()
        };
        c.facts.files = self.files.iter().cloned().collect();
        c.turn_start = self.turn_start;
        if self.turn_start.is_some() {
            if let Some((item, call)) = self.open_calls.values().max() {
                if let Some(ChatItem {
                    kind: ItemKind::Tools(calls),
                    ..
                }) = self.items.get(*item)
                {
                    c.activity = calls.get(*call).map(|c| c.title.clone());
                }
            }
        }
        c
    }

    fn item(&mut self, p: &Value, date: Option<Timestamp>, ordinal: &str) {
        let id = str_of(p, "id").map_or_else(|| format!("codex-{ordinal}"), String::from);
        match str_of(p, "type") {
            Some("message") => match str_of(p, "role") {
                Some("user") => {
                    let Some(text) = user_text(p) else { return };
                    let images = p.get("content").and_then(Value::as_array).map_or(0, |c| {
                        c.iter()
                            .filter(|b| str_of(b, "type") == Some("input_image"))
                            .count()
                    });
                    self.items.push(ChatItem {
                        id,
                        kind: ItemKind::User { text, images },
                        date,
                    });
                }
                Some("assistant") => {
                    let parts: Vec<&str> = p
                        .get("content")
                        .and_then(Value::as_array)
                        .map(|c| c.iter().filter_map(|b| str_of(b, "text")).collect())
                        .unwrap_or_default();
                    let text = parts.join("\n").trim().to_string();
                    if !text.is_empty() {
                        self.items.push(ChatItem {
                            id,
                            kind: ItemKind::Assistant { text },
                            date,
                        });
                    }
                }
                _ => {}
            },
            Some("reasoning") => {
                let parts: Vec<&str> = p
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|c| c.iter().filter_map(|b| str_of(b, "text")).collect())
                    .unwrap_or_default();
                let text = parts.join("\n\n").trim().to_string();
                if !text.is_empty() {
                    self.items.push(ChatItem {
                        id,
                        kind: ItemKind::Thinking { seconds: 0.0, text },
                        date,
                    });
                }
            }
            Some("function_call" | "custom_tool_call" | "local_shell_call") => {
                let call_id = str_of(p, "call_id").unwrap_or(&id).to_string();
                let name = str_of(p, "name").unwrap_or("shell");
                let raw = str_of(p, "arguments")
                    .or_else(|| str_of(p, "input"))
                    .unwrap_or("");
                self.add(call(&call_id, name, raw, p.get("action")), date);
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                let Some(call_id) = str_of(p, "call_id") else {
                    return;
                };
                let Some((item, index)) = self.open_calls.remove(call_id) else {
                    return;
                };
                let Some(ChatItem {
                    kind: ItemKind::Tools(calls),
                    ..
                }) = self.items.get_mut(item)
                else {
                    return;
                };
                let Some(call) = calls.get_mut(index) else {
                    return;
                };
                let output = p.get("output").map(output_text).unwrap_or_default();
                call.output = Some(if output.chars().count() > 20_000 {
                    format!("{}\n…", prefix_chars(&output, 20_000))
                } else {
                    output.clone()
                });
                call.finished = true;
                if exit_code(&output).is_some_and(|n| n != 0) {
                    call.is_error = true;
                }
            }
            _ => {}
        }
    }

    fn add(&mut self, call: ToolCall, date: Option<Timestamp>) {
        if call.kind == ToolKind::Shell {
            self.facts.commands += 1;
        }
        if call.kind == ToolKind::Edit {
            self.files.extend(
                call.input
                    .split(['\n', '\r'])
                    .filter(|l| !l.is_empty())
                    .map(String::from),
            );
        }
        let call_id = call.id.clone();
        let last = self.items.len().saturating_sub(1);
        if let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.items.last_mut()
        {
            calls.push(call);
            let at = (last, calls.len() - 1);
            self.open_calls.insert(call_id, at);
        } else {
            self.items.push(ChatItem {
                id: format!("tools-{call_id}"),
                kind: ItemKind::Tools(vec![call]),
                date,
            });
            self.open_calls.insert(call_id, (self.items.len() - 1, 0));
        }
    }

    fn event(&mut self, p: &Value, date: Option<Timestamp>, ordinal: &str) {
        match str_of(p, "type") {
            Some("task_started") => self.turn_start = Some(date.unwrap_or_else(now_secs)),
            Some("task_complete") => {
                let seconds = p
                    .get("duration_ms")
                    .and_then(Value::as_f64)
                    .map(|ms| ms / 1000.0)
                    .or_else(|| self.turn_start.zip(date).map(|(start, end)| end - start))
                    .unwrap_or(0.0);
                let model = self.model.clone();
                self.items.push(ChatItem {
                    id: format!("end-{ordinal}"),
                    kind: ItemKind::TurnEnd { seconds, model },
                    date,
                });
                self.facts.turns += 1;
                self.turn_start = None;
                self.open_calls.clear();
            }
            Some("turn_aborted") => {
                self.items.push(ChatItem {
                    id: format!("stop-{ordinal}"),
                    kind: ItemKind::notice("Stopped", "stop.circle"),
                    date,
                });
                self.turn_start = None;
                self.open_calls.clear();
            }
            Some("token_count") => {
                let Some(info) = p.get("info").filter(|i| i.is_object()) else {
                    return;
                };
                if let Some(total) = info.get("total_token_usage").filter(|t| t.is_object()) {
                    if let Some(v) = total.get("input_tokens") {
                        self.facts.input_tokens = count_of(v);
                    }
                    if let Some(v) = total.get("output_tokens") {
                        self.facts.output_tokens = count_of(v);
                    }
                    if let Some(v) = total.get("cached_input_tokens") {
                        self.facts.cached_tokens = count_of(v);
                    }
                }
                if let Some(last) = info.get("last_token_usage").filter(|t| t.is_object()) {
                    let n = |k: &str| last.get(k).map_or(0, count_of);
                    self.facts.context_tokens =
                        n("input_tokens").saturating_add(n("output_tokens"));
                }
            }
            Some("error") => {
                let text = str_of(p, "message").unwrap_or("Codex hit an error.");
                self.items.push(ChatItem {
                    id: format!("error-{ordinal}"),
                    kind: ItemKind::notice(text, "exclamationmark.triangle"),
                    date,
                });
            }
            _ => {}
        }
    }
}

impl LineParser for Parser {
    fn consume(&mut self, line: &[u8]) {
        let Some(map) = parse_object(line) else {
            return;
        };
        let obj = Value::Object(map);
        let Some(payload) = obj.get("payload").filter(|p| p.is_object()) else {
            return;
        };
        let date = date_field(&obj);
        let ordinal = match obj.get("ordinal") {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => {
                self.fallback_ids += 1;
                format!("n{}", self.fallback_ids)
            }
        };
        match str_of(&obj, "type") {
            Some("turn_context") => {
                if let Some(m) = str_of(payload, "model") {
                    self.model = Some(m.to_string());
                    self.facts.model = Some(m.to_string());
                }
                let settings = payload
                    .get("collaboration_mode")
                    .and_then(|m| m.get("settings"));
                self.facts.effort = settings
                    .and_then(|s| str_of(s, "reasoning_effort"))
                    .or_else(|| str_of(payload, "effort"))
                    .map(String::from);
            }
            Some("response_item") => self.item(payload, date, &ordinal),
            Some("event_msg") => self.event(payload, date, &ordinal),
            _ => {}
        }
    }
}

fn cursors() -> &'static Cursors<Parser, Arc<Conversation>> {
    static CURSORS: OnceLock<Cursors<Parser, Arc<Conversation>>> = OnceLock::new();
    CURSORS.get_or_init(|| {
        Cursors::chat(30, |p: &Parser| Arc::new(p.conversation())).long_lines_free(64_000_000)
    })
}

pub fn read_shared(path: &str) -> Arc<Conversation> {
    cursors().read(path).unwrap_or_default()
}

pub fn read(path: &str) -> Conversation {
    (*read_shared(path)).clone()
}
