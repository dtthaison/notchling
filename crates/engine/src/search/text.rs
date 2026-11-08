//! What text a transcript line contributes to the index.

use serde_json::{Map, Value};

/// Lines longer than this are skipped, like the transcript reader does (they hold base64 images).
pub const MAX_LINE_BYTES: usize = 100_000;
/// One message contributes at most this many characters, so a pasted log can't crowd out the rest.
pub const MAX_MESSAGE_CHARS: usize = 6_000;

const CLAUDE_MARKERS: [&[u8]; 4] = [
    br#""role":"user""#,
    br#""role":"assistant""#,
    br#""customTitle""#,
    br#""aiTitle""#,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRole {
    User,
    Assistant,
}

/// A transcript line as far as search is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptLine {
    Message {
        role: SessionRole,
        text: String,
        cwd: Option<String>,
    },
    CustomTitle(String),
    AiTitle(String),
}

/// A byte search that doesn't parse: most lines of a transcript are tool output and fail it.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    match needle.first() {
        None => true,
        Some(&first) => haystack
            .iter()
            .enumerate()
            .any(|(i, &b)| b == first && haystack[i..].starts_with(needle)),
    }
}

fn str_of<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

/// The elements as objects, or `None` when the value is not an array of only objects (Swift's
/// `as? [[String: Any]]`).
fn object_array(value: Option<&Value>) -> Option<Vec<&Map<String, Value>>> {
    value?.as_array()?.iter().map(Value::as_object).collect()
}

/// The `text` of every part whose `type` is `kind`, joined with newlines.
fn part_text(parts: &[&Map<String, Value>], kind: &str) -> String {
    parts
        .iter()
        .filter(|p| str_of(p, "type") == Some(kind))
        .filter_map(|p| str_of(p, "text"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Claude Code transcript line: what you typed and what Claude said (text only, no thinking or
/// tool calls), and the titles. Meta lines, sidechains and the slash-command and system text Claude
/// Code stores as user lines are left out.
pub fn claude_line(line: &[u8]) -> Option<TranscriptLine> {
    if line.len() > MAX_LINE_BYTES || !CLAUDE_MARKERS.iter().any(|m| contains(line, m)) {
        return None;
    }
    let value: Value = serde_json::from_slice(line).ok()?;
    let obj = value.as_object()?;
    match str_of(obj, "type")? {
        "custom-title" => {
            return str_of(obj, "customTitle")
                .filter(|t| !t.is_empty())
                .map(|t| TranscriptLine::CustomTitle(t.to_string()))
        }
        "ai-title" => {
            return str_of(obj, "aiTitle")
                .filter(|t| !t.is_empty())
                .map(|t| TranscriptLine::AiTitle(t.to_string()))
        }
        "user" | "assistant" => {}
        _ => return None,
    }
    if obj.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || obj.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let message = obj.get("message")?.as_object()?;
    let role = if str_of(message, "role")? == "assistant" {
        SessionRole::Assistant
    } else {
        SessionRole::User
    };
    let content = message.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        other => part_text(&object_array(Some(other))?, "text"),
    };
    let clean = clean(&text, role)?;
    Some(TranscriptLine::Message {
        role,
        text: clean,
        cwd: str_of(obj, "cwd").map(str::to_string),
    })
}

/// A Codex rollout line: `response_item` messages, leaving out the developer prompt and the
/// environment and AGENTS.md text Codex injects as user messages.
pub fn codex_line(line: &[u8]) -> Option<TranscriptLine> {
    if line.len() > MAX_LINE_BYTES || !contains(line, br#""message""#) {
        return None;
    }
    let value: Value = serde_json::from_slice(line).ok()?;
    let obj = value.as_object()?;
    if str_of(obj, "type")? != "response_item" {
        return None;
    }
    let payload = obj.get("payload")?.as_object()?;
    if str_of(payload, "type")? != "message" {
        return None;
    }
    let parts = object_array(payload.get("content")).unwrap_or_default();
    match str_of(payload, "role")? {
        "user" => {
            let text = codex_user_text(&parts)?;
            clean(&text, SessionRole::User).map(|text| TranscriptLine::Message {
                role: SessionRole::User,
                text,
                cwd: None,
            })
        }
        "assistant" => {
            clean(&part_text(&parts, "output_text"), SessionRole::Assistant).map(|text| {
                TranscriptLine::Message {
                    role: SessionRole::Assistant,
                    text,
                    cwd: None,
                }
            })
        }
        _ => None,
    }
}

/// What the user typed in a Codex message; `None` for the context Codex injects.
fn codex_user_text(parts: &[&Map<String, Value>]) -> Option<String> {
    let text = part_text(parts, "input_text");
    let text = text.trim();
    if text.is_empty() || text.starts_with('<') || text.starts_with("# AGENTS.md") {
        return None;
    }
    Some(text.to_string())
}

/// Trimmed and capped (in characters); `None` for text that isn't something the conversation said.
pub fn clean(text: &str, role: SessionRole) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if role == SessionRole::User {
        // Slash commands, their output, caveats, system reminders and hook text are wrapped in tags.
        if t.starts_with('<') || t.starts_with("Caveat:") || t.starts_with("[Request interrupted") {
            return None;
        }
    }
    Some(t.chars().take(MAX_MESSAGE_CHARS).collect())
}

fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// The first non-empty line of a prompt, at most 80 characters (79 and an ellipsis when longer).
pub fn first_line_title(text: &str) -> String {
    let line = text
        .split(is_newline)
        .find(|l| !l.is_empty())
        .unwrap_or(text);
    if line.chars().count() > 80 {
        let mut s: String = line.chars().take(79).collect();
        s.push('…');
        s
    } else {
        line.to_string()
    }
}
