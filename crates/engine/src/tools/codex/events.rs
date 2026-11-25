use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ThreadStarted { thread_id: String },
    TurnStarted,
    TurnCompleted,
    Failed { message: Option<String> },
    Item { phase: String, kind: String },
    Other,
}

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
