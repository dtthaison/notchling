//! What the notch shows about one agent, read from its transcript tail: the last messages, the tools
//! since the last text, the current activity and the last question asked.

use super::subagents;
use super::tail::{Cursors, LineParser, LONG_LINE_SCAN, MAX_LINE_BYTES, TAIL_BYTES};
use super::tally::{describe, ToolTally};
use super::text::{collapse_pastes, suffix_chars};
use super::util::{now_secs, parse_object, str_of};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    User,
    Assistant,
    Tools,
}

/// One entry of the conversation, as the terminal shows it: a prompt, Claude's text, or a one-line
/// summary of the tools it ran in between ("Read 2 files, ran 1 shell command").
#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub id: usize,
    pub kind: LineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionItem {
    pub header: String,
    pub question: String,
    pub options: Vec<String>,
    /// What each option means, as Claude wrote it; empty when unknown.
    pub details: Vec<String>,
    pub multi_select: bool,
}

/// A question the agent asked with AskUserQuestion that hasn't been answered yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub tool_use_id: String,
    pub items: Vec<QuestionItem>,
}

impl PendingQuestion {
    /// Answering from the notch covers single-choice questions; multi-select needs the terminal.
    pub fn answerable(&self) -> bool {
        !self.items.is_empty() && !self.items.iter().any(|i| i.multi_select)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub lines: Vec<ChatLine>,
    pub last_prompt: Option<String>,
    pub activity: Option<String>,
    pub question: Option<PendingQuestion>,
    /// Subagents that wrote to their transcripts in the last minute and a half.
    pub subagents_active: usize,
}

impl Snapshot {
    pub fn last_reply(&self) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.kind == LineKind::Assistant)
            .map(|l| l.text.as_str())
    }
}

/// The AskUserQuestion input as a pending question.
pub fn question(id: &str, input: &Map<String, Value>) -> Option<PendingQuestion> {
    let questions = input.get("questions")?.as_array()?;
    let items = questions
        .iter()
        .map(|q| {
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            QuestionItem {
                header: str_of(q, "header").unwrap_or("").to_string(),
                question: str_of(q, "question").unwrap_or("").to_string(),
                options: options
                    .iter()
                    .filter_map(|o| str_of(o, "label"))
                    .map(String::from)
                    .collect(),
                details: options
                    .iter()
                    .map(|o| str_of(o, "description").unwrap_or("").to_string())
                    .collect(),
                multi_select: q
                    .get("multiSelect")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }
        })
        .collect();
    Some(PendingQuestion {
        tool_use_id: id.to_string(),
        items,
    })
}

const KEPT_MESSAGES: usize = 120;
/// A message is shown as its last 3000 characters, so longer runs are cut down while kept.
const KEPT_BYTES: usize = 16_000;

/// Turns transcript lines into the snapshot, one line at a time, so a cursor can keep it between reads.
#[derive(Default)]
pub struct SnapshotParser {
    messages: Vec<(LineKind, String)>,
    /// Tool calls since the last piece of text, summed up when the next text arrives.
    tools: ToolTally,
    activity: Option<String>,
    last_prompt: Option<String>,
    last_question: Option<PendingQuestion>,
    last_question_answered: bool,
}

impl LineParser for SnapshotParser {
    fn consume(&mut self, line: &[u8]) {
        // Screenshots and long tool output: nothing the chat shows, and slow to decode.
        if line.len() > MAX_LINE_BYTES {
            return;
        }
        let Some(map) = parse_object(line) else {
            return;
        };
        let obj = Value::Object(map);
        let Some(kind) = str_of(&obj, "type").filter(|t| *t == "user" || *t == "assistant") else {
            return;
        };
        if obj.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || obj.get("isMeta").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let Some(message) = obj.get("message").filter(|m| m.is_object()) else {
            return;
        };

        let mut text = String::new();
        if let Some(s) = str_of(message, "content") {
            text = s.to_string();
        } else if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                match str_of(block, "type") {
                    Some("text") => {
                        if let Some(t) = str_of(block, "text") {
                            if !text.is_empty() {
                                text.push_str("\n\n");
                            }
                            text.push_str(t);
                        }
                    }
                    Some("tool_use") => self.tool_use(block),
                    Some("tool_result") => {
                        let id = str_of(block, "tool_use_id");
                        if id.is_some()
                            && id == self.last_question.as_ref().map(|q| q.tool_use_id.as_str())
                        {
                            self.last_question_answered = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        let text = collapse_pastes(&text).trim().to_string();
        // Skip injected wrappers like <command-name>, <system-reminder> and interrupt markers.
        if text.is_empty() || text.starts_with('<') || text.starts_with("[Request interrupted") {
            return;
        }
        if kind == "assistant" {
            self.activity = None;
        }
        self.flush_tools();
        let line_kind = if kind == "user" {
            LineKind::User
        } else {
            LineKind::Assistant
        };
        match self.messages.last_mut() {
            Some((last_kind, last_text)) if *last_kind == line_kind => {
                last_text.push_str("\n\n");
                last_text.push_str(&text);
            }
            _ => self.messages.push((line_kind, text)),
        }
        if line_kind == LineKind::User {
            self.last_prompt = self.messages.last().map(|m| m.1.clone());
        }
        self.trim();
    }
}

impl SnapshotParser {
    fn tool_use(&mut self, block: &Value) {
        let name = str_of(block, "name").unwrap_or("");
        let empty = Map::new();
        let input = block
            .get("input")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        self.activity = Some(describe(name, input));
        self.tools.add(name, input);
        if name == "AskUserQuestion" {
            if let Some(id) = str_of(block, "id") {
                if let Some(q) = question(id, input) {
                    self.last_question = Some(q);
                    self.last_question_answered = false;
                }
            }
        }
    }

    fn flush_tools(&mut self) {
        if let Some(summary) = self.tools.summary() {
            self.messages.push((LineKind::Tools, summary));
        }
        self.tools = ToolTally::default();
    }

    fn trim(&mut self) {
        if self.messages.len() > KEPT_MESSAGES * 2 {
            let extra = self.messages.len() - KEPT_MESSAGES;
            self.messages.drain(..extra);
        }
        if let Some((kind, text)) = self.messages.last_mut() {
            if *kind != LineKind::User && text.len() > KEPT_BYTES {
                *text = suffix_chars(text, 3001).to_string();
            }
        }
    }

    /// The snapshot as of the lines read so far. Tools still pending show as the last line, but stay
    /// open so the next lines can add to them.
    pub fn snapshot(&self) -> Snapshot {
        let mut shown = self.messages.clone();
        if let Some(summary) = self.tools.summary() {
            shown.push((LineKind::Tools, summary));
        }
        let start = shown.len().saturating_sub(40);
        let lines = shown[start..]
            .iter()
            .enumerate()
            .map(|(id, (kind, text))| {
                let text = if text.chars().count() > 3000 {
                    format!("…{}", suffix_chars(text, 3000))
                } else {
                    text.clone()
                };
                ChatLine {
                    id,
                    kind: *kind,
                    text,
                }
            })
            .collect();
        Snapshot {
            lines,
            last_prompt: self.last_prompt.clone(),
            activity: self.activity.clone(),
            question: if self.last_question_answered {
                None
            } else {
                self.last_question.clone()
            },
            subagents_active: 0,
        }
    }
}

fn cursors() -> &'static Cursors<SnapshotParser, Snapshot> {
    static CURSORS: OnceLock<Cursors<SnapshotParser, Snapshot>> = OnceLock::new();
    // The last 2 MB of lines it reads: pictures (over its own 100 KB cap) don't use the window.
    CURSORS.get_or_init(|| {
        Cursors::new(TAIL_BYTES, 300, SnapshotParser::snapshot)
            .skipping_lines_over(MAX_LINE_BYTES, LONG_LINE_SCAN)
    })
}

/// The snapshot of a transcript. The first call parses the last 2 MB of lines (pictures don't count); later calls parse only the lines
/// appended since. Subagents run on while the session's own transcript stays still, so their count
/// is taken fresh each call.
pub fn read(path: &str) -> Snapshot {
    let mut snapshot = cursors().read(path).unwrap_or_default();
    snapshot.subagents_active = subagents::active_count(path, now_secs());
    snapshot
}

/// The last `limit` lines of the snapshot.
pub fn lines(path: &str, limit: usize) -> Vec<ChatLine> {
    let all = read(path).lines;
    let start = all.len().saturating_sub(limit);
    all[start..].to_vec()
}
