pub mod images;
mod steps;
mod window;

use images::ImageRef;
pub use steps::{cap_output, result_text, step, tagged, DiffRow, DiffRowKind, ToolStep};
pub use window::{read_window, strip_base64};

use super::model::Timestamp;
use super::tail::MAX_LINE_BYTES;
use super::tally::ToolTally;
use super::text::{collapse_pastes, prefix_chars};
use super::typed::typed_prompt;
use super::util::{date_field, parse_object, str_of};
use serde_json::{Map, Value};
use std::collections::HashMap;

pub const TAIL_BYTES: u64 = 2_000_000;
pub const MAX_TEXT_CHARACTERS: usize = 3000;
pub const DEFAULT_LIMIT: usize = 200;
const DISTANT_PAST: Timestamp = -62_135_769_600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    Tools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Text,
    Shell,
    Compaction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub text: String,
    pub timestamp: Timestamp,
    pub tool_summary: Option<String>,
    pub queued: bool,
    pub source_offset: Option<u64>,
    pub kind: ChatKind,
    pub steps: Vec<ToolStep>,
    pub output: Option<String>,
    pub images: Vec<ImageRef>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParseResult {
    pub messages: Vec<ChatMessage>,
    pub malformed_lines: usize,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceLine {
    pub offset: u64,
    pub text: String,
}

pub fn read(path: &str, is_main: bool, limit: usize) -> ParseResult {
    let home = notchling_platform::paths::home().unwrap_or_default();
    read_in(path, is_main, limit, &home)
}

pub fn read_in(path: &str, is_main: bool, limit: usize, home: &std::path::Path) -> ParseResult {
    match read_window(path, TAIL_BYTES) {
        Some(lines) => parse_source(&lines, is_main, path, limit, home),
        None => ParseResult::default(),
    }
}

pub fn tail(path: &str, bytes: u64) -> Option<Vec<String>> {
    read_window(path, bytes).map(|l| l.into_iter().map(|s| s.text).collect())
}

pub fn parse(lines: &[String], is_main: bool, path: &str, limit: usize) -> ParseResult {
    let source: Vec<SourceLine> = lines
        .iter()
        .enumerate()
        .map(|(i, t)| SourceLine {
            offset: i as u64,
            text: t.clone(),
        })
        .collect();
    let home = notchling_platform::paths::home().unwrap_or_default();
    parse_source(&source, is_main, path, limit, &home)
}

struct Draft {
    role: ChatRole,
    text: String,
    timestamp: Timestamp,
    seed: String,
    tool_summary: Option<String>,
    offset: u64,
    queued: bool,
    kind: ChatKind,
    steps: Vec<ToolStep>,
    output: Option<String>,
    images: Vec<ImageRef>,
}

impl Draft {
    fn new(role: ChatRole, text: String, timestamp: Timestamp, seed: String, offset: u64) -> Draft {
        Draft {
            role,
            text,
            timestamp,
            seed,
            tool_summary: None,
            offset,
            queued: false,
            kind: ChatKind::Text,
            steps: Vec::new(),
            output: None,
            images: Vec::new(),
        }
    }
}

struct Waiting {
    content: String,
    timestamp: Timestamp,
    offset: u64,
}

#[derive(Default)]
struct State {
    drafts: Vec<Draft>,
    waiting: Vec<Waiting>,
    dequeue_seen: bool,
    taken: Vec<(String, Timestamp)>,
    tally: ToolTally,
    tally_timestamp: Timestamp,
    tally_seed: String,
    tally_offset: u64,
    tally_steps: Vec<ToolStep>,
    tally_index: HashMap<String, usize>,
    flushed_steps: HashMap<String, (usize, usize)>,
    tally_images: Vec<ImageRef>,
}

impl State {
    fn flush_tools(&mut self) {
        if let Some(summary) = self.tally.summary() {
            let mut d = Draft::new(
                ChatRole::Tools,
                summary.clone(),
                self.tally_timestamp,
                format!("{}:tools", self.tally_seed),
                self.tally_offset,
            );
            d.tool_summary = Some(summary);
            d.steps = std::mem::take(&mut self.tally_steps);
            d.images = std::mem::take(&mut self.tally_images);
            for (index, step) in d.steps.iter().enumerate() {
                self.flushed_steps
                    .insert(step.id.clone(), (self.drafts.len(), index));
            }
            self.drafts.push(d);
        } else if !self.tally_images.is_empty() {
            let pictures = std::mem::take(&mut self.tally_images);
            if let Some(last) = self.drafts.last_mut() {
                last.images.extend(pictures);
            }
        }
        self.tally_images.clear();
        self.tally_steps.clear();
        self.tally_index.clear();
        self.tally = ToolTally::default();
    }

    fn attach_result(&mut self, id: &str, output: String) {
        if output.is_empty() {
            return;
        }
        if let Some(&index) = self.tally_index.get(id) {
            self.tally_steps[index].output = Some(output);
        } else if let Some(&(draft, index)) = self.flushed_steps.get(id) {
            if let Some(step) = self
                .drafts
                .get_mut(draft)
                .and_then(|d| d.steps.get_mut(index))
            {
                step.output = Some(output);
            }
        }
    }

    fn add_user(
        &mut self,
        text: String,
        timestamp: Timestamp,
        seed: String,
        offset: u64,
        merge: bool,
        images: Vec<ImageRef>,
    ) {
        self.flush_tools();
        if merge {
            if let Some(last) = self
                .drafts
                .last_mut()
                .filter(|d| d.role == ChatRole::User && !d.queued && d.kind == ChatKind::Text)
            {
                if !text.is_empty() {
                    if !last.text.is_empty() {
                        last.text.push_str("\n\n");
                    }
                    last.text.push_str(&text);
                }
                last.images.extend(images);
                return;
            }
        }
        let mut draft = Draft::new(ChatRole::User, text, timestamp, seed, offset);
        draft.images = images;
        self.drafts.push(draft);
    }

    fn was_taken(&self, text: &str, near: Timestamp) -> bool {
        self.taken
            .iter()
            .any(|(content, time)| content == text && (time - near).abs() < 120.0)
    }

    fn take_waiting(&mut self, content: &str) {
        if let Some(i) = self.waiting.iter().position(|w| w.content == content) {
            self.waiting.remove(i);
        }
    }
}

fn cap(text: &str) -> String {
    if text.chars().count() > MAX_TEXT_CHARACTERS {
        format!("{}…", prefix_chars(text, MAX_TEXT_CHARACTERS - 1))
    } else {
        text.to_string()
    }
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn parse_source(
    lines: &[SourceLine],
    is_main: bool,
    path: &str,
    limit: usize,
    home: &std::path::Path,
) -> ParseResult {
    let mut st = State::default();
    let mut malformed = 0;
    let mut latest_model: Option<String> = None;

    for (index, source) in lines.iter().enumerate() {
        let (text, stripped) = images::strip_base64_blocks(&source.text);
        if text.len() > MAX_LINE_BYTES {
            continue;
        }
        let Some(map) = parse_object(text.as_bytes()) else {
            malformed += 1;
            continue;
        };
        let object = Value::Object(map);
        let timestamp = date_field(&object).unwrap_or(DISTANT_PAST);
        let kind = str_of(&object, "type");

        if kind == Some("queue-operation") {
            let content = str_of(&object, "content").unwrap_or("").trim().to_string();
            st.dequeue_seen = str_of(&object, "operation") == Some("dequeue");
            match str_of(&object, "operation") {
                Some("enqueue") => {
                    if !content.is_empty() && !content.starts_with('<') {
                        st.waiting.push(Waiting {
                            content,
                            timestamp,
                            offset: source.offset,
                        });
                    }
                }
                Some("dequeue") => {
                    if !st.waiting.is_empty() {
                        st.waiting.remove(0);
                    }
                }
                Some("popAll") => st.take_waiting(&content),
                Some("remove") if !content.is_empty() && !content.starts_with('<') => {
                    st.take_waiting(&content);
                    if !st.was_taken(&content, timestamp) {
                        let seed = str_of(&object, "commandUuid")
                            .map_or_else(|| format!("{path}:q{index}"), String::from);
                        st.add_user(
                            collapse_pastes(&content),
                            timestamp,
                            seed,
                            source.offset,
                            false,
                            Vec::new(),
                        );
                        st.taken.push((content, timestamp));
                    }
                }
                _ => {}
            }
            continue;
        }

        if kind == Some("attachment") {
            if let Some(attachment) = object
                .get("attachment")
                .filter(|a| str_of(a, "type") == Some("queued_command"))
            {
                let origin = attachment.get("origin").and_then(|o| str_of(o, "kind"));
                let mut text = String::new();
                match attachment.get("prompt") {
                    Some(Value::String(s)) => text = s.clone(),
                    Some(Value::Array(blocks)) => {
                        text = blocks
                            .iter()
                            .filter(|b| str_of(b, "type") == Some("text"))
                            .filter_map(|b| str_of(b, "text"))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                    }
                    _ => {}
                }
                let text = collapse_pastes(&text).trim().to_string();
                let pictures: Vec<ImageRef> =
                    images::refs(attachment.get("prompt"), source.offset, &stripped)
                        .into_iter()
                        .map(|(image, _)| image)
                        .collect();
                if !(origin.is_none() || origin == Some("human"))
                    || (text.is_empty() && pictures.is_empty())
                    || text.starts_with('<')
                {
                    continue;
                }
                st.take_waiting(&text);
                if st.was_taken(&text, timestamp) {
                    continue;
                }
                let seed = str_of(&object, "uuid")
                    .map_or_else(|| format!("{path}:a{index}"), String::from);
                st.add_user(
                    text.clone(),
                    timestamp,
                    seed,
                    source.offset,
                    false,
                    pictures,
                );
                st.taken.push((text, timestamp));
                continue;
            }
        }

        if kind == Some("system") && str_of(&object, "subtype") == Some("compact_boundary") {
            st.flush_tools();
            let before = object
                .get("compactMetadata")
                .and_then(|m| m.get("preTokens"))
                .and_then(Value::as_i64);
            let note = before.map_or(String::new(), |n| {
                if n >= 1000 {
                    format!("{}k tokens", n / 1000)
                } else {
                    format!("{n} tokens")
                }
            });
            let seed =
                str_of(&object, "uuid").map_or_else(|| format!("{path}:c{index}"), String::from);
            let mut d = Draft::new(ChatRole::Assistant, note, timestamp, seed, source.offset);
            d.kind = ChatKind::Compaction;
            st.drafts.push(d);
            continue;
        }
        if object.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        let Some(role) = kind.filter(|k| *k == "user" || *k == "assistant") else {
            continue;
        };
        let Some(message) = object.get("message").filter(|m| m.is_object()) else {
            continue;
        };
        if is_main
            && (object.get("isSidechain").and_then(Value::as_bool) == Some(true)
                || object.get("isMeta").and_then(Value::as_bool) == Some(true))
        {
            continue;
        }
        if role == "assistant" {
            if let Some(model) = str_of(message, "model").filter(|m| !m.starts_with('<')) {
                latest_model = Some(model.to_string());
            }
        }
        let seed = str_of(&object, "uuid").map_or_else(|| format!("{path}:{index}"), String::from);

        let mut text = String::new();
        if let Some(plain) = str_of(message, "content") {
            text = plain.to_string();
        } else if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                match str_of(block, "type") {
                    Some("text") => {
                        if let Some(part) = str_of(block, "text") {
                            if !text.is_empty() {
                                text.push_str("\n\n");
                            }
                            text.push_str(part);
                        }
                    }
                    Some("tool_use") => {
                        if st.tally.is_empty() {
                            st.tally_timestamp = timestamp;
                            st.tally_seed = seed.clone();
                            st.tally_offset = source.offset;
                        }
                        let name = str_of(block, "name").unwrap_or("");
                        let empty = Map::new();
                        let input = block
                            .get("input")
                            .and_then(Value::as_object)
                            .unwrap_or(&empty);
                        st.tally.add(name, input);
                        let call_id = str_of(block, "id").map_or_else(
                            || format!("{seed}:{}", st.tally_steps.len()),
                            String::from,
                        );
                        st.tally_index.insert(call_id.clone(), st.tally_steps.len());
                        st.tally_steps.push(step(&call_id, name, input));
                    }
                    Some("tool_result") => {
                        if let Some(call_id) = str_of(block, "tool_use_id") {
                            st.attach_result(call_id, result_text(block.get("content")));
                        }
                    }
                    _ => {}
                }
            }
        }
        let text = collapse_pastes(&text).trim().to_string();
        let (own, tool_pictures): (Vec<_>, Vec<_>) =
            images::refs(message.get("content"), source.offset, &stripped)
                .into_iter()
                .partition(|(_, in_tool_result)| !in_tool_result);
        let (text, marked) = images::file_markers(&text, home);
        let mut pictures: Vec<ImageRef> = own.into_iter().map(|(image, _)| image).collect();
        pictures.extend(marked);
        st.tally_images
            .extend(tool_pictures.into_iter().map(|(image, _)| image));

        if role == "user" {
            if !text.is_empty() {
                let after_dequeue = std::mem::take(&mut st.dequeue_seen);
                if !after_dequeue && typed_prompt(&object, &text) {
                    st.waiting.clear();
                }
            }
            if let Some(command) = tagged(&text, "bash-input") {
                st.take_waiting(&command);
                st.flush_tools();
                let mut d = Draft::new(
                    ChatRole::User,
                    format!("!{command}"),
                    timestamp,
                    seed,
                    source.offset,
                );
                d.kind = ChatKind::Shell;
                st.drafts.push(d);
                continue;
            }
            if text.contains("<bash-stdout>") || text.contains("<bash-stderr>") {
                let out = [tagged(&text, "bash-stdout"), tagged(&text, "bash-stderr")]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(last) = st.drafts.last_mut().filter(|d| d.kind == ChatKind::Shell) {
                    last.output = Some(cap_output(&out));
                }
                continue;
            }
        }
        if (text.is_empty() && pictures.is_empty())
            || text.starts_with('<')
            || text.starts_with("[Request interrupted")
        {
            continue;
        }

        if role == "user" {
            st.take_waiting(&text);
            if st.was_taken(&text, timestamp) {
                continue;
            }
            st.add_user(text, timestamp, seed, source.offset, true, pictures);
            continue;
        }
        st.flush_tools();
        if let Some(last) = st
            .drafts
            .last_mut()
            .filter(|d| d.role == ChatRole::Assistant && d.kind == ChatKind::Text)
        {
            if !text.is_empty() {
                last.text.push_str("\n\n");
                last.text.push_str(&text);
            }
            last.images.extend(pictures);
        } else {
            let mut draft = Draft::new(ChatRole::Assistant, text, timestamp, seed, source.offset);
            draft.images = pictures;
            st.drafts.push(draft);
        }
    }
    st.flush_tools();
    for item in std::mem::take(&mut st.waiting) {
        let seed = format!("queued:{}:{:x}", item.offset, fnv(&item.content));
        let mut d = Draft::new(
            ChatRole::User,
            collapse_pastes(&item.content),
            item.timestamp,
            seed,
            item.offset,
        );
        d.queued = true;
        st.drafts.push(d);
    }

    let start = st.drafts.len().saturating_sub(limit);
    let messages = st
        .drafts
        .drain(start..)
        .map(|d| ChatMessage {
            id: d.seed,
            role: d.role,
            text: cap(&d.text),
            timestamp: d.timestamp,
            tool_summary: d.tool_summary,
            queued: d.queued,
            source_offset: Some(d.offset),
            kind: d.kind,
            steps: d.steps,
            output: d.output,
            images: d.images,
        })
        .collect();
    ParseResult {
        messages,
        malformed_lines: malformed,
        model: latest_model,
    }
}
