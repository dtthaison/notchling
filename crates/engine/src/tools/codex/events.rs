//! The lines `codex exec --json` prints, one JSON object each. The Swift runner reads three:
//! `thread.started` (the new session's id), `error` and `turn.failed` (what went wrong). The rest
//! are named so the daemon can say a turn is making progress, but nothing depends on them: Chat
//! reads the conversation from the session's rollout file, as for any Codex session.

use serde_json::Value;

/// One `exec --json` event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// `{"type":"thread.started","thread_id":"…"}`: the session's id.
    ThreadStarted {
        thread_id: String,
    },
    TurnStarted,
    /// `{"type":"turn.completed","usage":{…}}`.
    TurnCompleted,
    /// `turn.failed` (`{"error":{"message"}}`) or `error` (`{"message"}`).
    Failed {
        message: Option<String>,
    },
    /// `item.started`, `item.updated` or `item.completed`, with the item's type
    /// (`agent_message`, `command_execution`, `file_change`…).
    Item {
        phase: String,
        kind: String,
    },
    /// Anything else, or a line that isn't JSON.
    Other,
}

/// The longest line read: a line over this is dropped by the reader before it gets here, and
/// anything longer than it is not parsed.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

pub fn parse_line(line: &str) -> Event {
    if line.len() > MAX_LINE_BYTES {
        return Event::Other;
    }
    let Ok(Value::Object(event)) = serde_json::from_str::<Value>(line.trim()) else {
        return Event::Other;
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(String::from);
    let root = Value::Object(event);
    match text(&root, "type").as_deref() {
        Some("thread.started") => match text(&root, "thread_id") {
            Some(thread_id) if !thread_id.is_empty() => Event::ThreadStarted { thread_id },
            _ => Event::Other,
        },
        Some("turn.started") => Event::TurnStarted,
        Some("turn.completed") => Event::TurnCompleted,
        Some("error" | "turn.failed") => Event::Failed {
            message: text(&root, "message").or_else(|| {
                root.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(String::from)
            }),
        },
        Some(phase @ ("item.started" | "item.updated" | "item.completed")) => Event::Item {
            phase: phase.trim_start_matches("item.").to_string(),
            kind: root
                .get("item")
                .and_then(|i| text(i, "type"))
                .unwrap_or_default(),
        },
        _ => Event::Other,
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
