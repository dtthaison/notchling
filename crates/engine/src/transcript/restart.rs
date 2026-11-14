use super::tail::read_lines;
use super::util::{modified_secs, parse_object, str_of};
use serde_json::Value;

pub const WINDOW_BYTES: u64 = 1_000_000;
pub const STALE_MS: i64 = 600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    Over,
    Running,
    Unknown,
}

fn flag(obj: &Value, key: &str) -> bool {
    obj.get(key).and_then(Value::as_bool) == Some(true)
}

fn first_text(obj: &Value) -> String {
    match obj.pointer("/message/content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .find_map(|b| (str_of(b, "type") == Some("text")).then(|| str_of(b, "text")))
            .flatten()
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn carries_result(obj: &Value) -> bool {
    obj.pointer("/message/content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks
                .iter()
                .any(|b| str_of(b, "type") == Some("tool_result"))
        })
}

fn carries_image(obj: &Value) -> bool {
    obj.pointer("/message/content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| blocks.iter().any(|b| str_of(b, "type") == Some("image")))
}

fn user_line(obj: &Value) -> Option<Tail> {
    if carries_result(obj) {
        return Some(Tail::Running);
    }
    let text = first_text(obj);
    let text = text.trim_start();
    if text.starts_with("[Request interrupted") {
        return Some(Tail::Over);
    }
    if text.starts_with("<task-notification>") {
        return Some(Tail::Running);
    }
    let local = text.starts_with("<local-command-")
        || text.starts_with("<command-name>")
        || text.starts_with("<bash-");
    if local || flag(obj, "isMeta") || flag(obj, "isCompactSummary") {
        return None;
    }
    (!text.is_empty() || carries_image(obj)).then_some(Tail::Running)
}

pub fn classify<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Tail {
    for line in lines.rev() {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let obj = Value::Object(map);
        if flag(&obj, "isSidechain") {
            continue;
        }
        let verdict = match str_of(&obj, "type") {
            Some("system") => {
                (str_of(&obj, "subtype") == Some("turn_duration")).then_some(Tail::Over)
            }
            Some("user") => user_line(&obj),
            Some("assistant") => Some(if flag(&obj, "isApiErrorMessage") {
                Tail::Over
            } else {
                Tail::Running
            }),
            _ => None,
        };
        if let Some(verdict) = verdict {
            return verdict;
        }
    }
    Tail::Unknown
}

pub fn restarted(path: &str, now_ms: i64) -> bool {
    let Some(modified) = modified_secs(path) else {
        return false;
    };
    if now_ms - (modified * 1000.0) as i64 > STALE_MS {
        return false;
    }
    let Ok(lines) = read_lines(path, WINDOW_BYTES) else {
        return false;
    };
    if lines.is_empty() {
        return std::fs::metadata(path).is_ok_and(|m| m.len() > WINDOW_BYTES);
    }
    classify(lines.iter().map(String::as_str)) == Tail::Running
}

#[cfg(test)]
#[path = "restart_tests.rs"]
mod tests;
