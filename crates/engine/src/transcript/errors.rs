//! Reading what a transcript's end says: a usage-limit error, a reply cut by sleep, a tool call that
//! was answered somewhere else. Each reads only the last few hundred KB.

use super::model::Timestamp;
use super::tail::{read_lines, MAX_LINE_BYTES};
use super::util::{date_field, json_sorted, parse_object, str_of};
use serde_json::Value;
use std::collections::HashSet;

/// A usage-limit error ending a transcript: the entry's id and time, and what it said.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitHit {
    pub id: String,
    pub at: Option<Timestamp>,
    pub text: String,
}

fn user_or_assistant(line: &str) -> Option<Value> {
    if line.len() > MAX_LINE_BYTES {
        return None;
    }
    let obj = Value::Object(parse_object(line.as_bytes())?);
    matches!(str_of(&obj, "type"), Some("user" | "assistant")).then_some(obj)
}

/// The transcript's last message when it is the account's usage-limit error ("You've hit your session
/// limit - resets 7pm"), so nothing has happened since; None otherwise. The provider's own throttle
/// ("API Error: Request rejected (429) ... cooling down") is a different `error` and text, and doesn't
/// count.
pub fn limit_hit_in_file(path: &str) -> Option<LimitHit> {
    let lines = read_lines(path, 200_000).ok()?;
    for line in lines.iter().rev() {
        let Some(obj) = user_or_assistant(line) else {
            continue;
        };
        if obj.get("isApiErrorMessage").and_then(Value::as_bool) != Some(true)
            || str_of(&obj, "error") != Some("rate_limit")
        {
            return None;
        }
        let id = str_of(&obj, "uuid")?.to_string();
        let content = obj.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| str_of(b, "text"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !(text.starts_with("You've hit your") || text.starts_with("You\u{2019}ve hit your")) {
            return None;
        }
        return Some(LimitHit {
            id,
            at: date_field(&obj),
            text,
        });
    }
    None
}

/// The id of the transcript's last message when it is the error Claude Code writes after the Mac slept
/// mid-reply, so nothing has happened since; None otherwise.
pub fn sleep_cut_in_file(path: &str) -> Option<String> {
    let lines = read_lines(path, 200_000).ok()?;
    for line in lines.iter().rev() {
        let Some(obj) = user_or_assistant(line) else {
            continue;
        };
        let errored = obj.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true);
        let cut = str_of(&obj, "streamCutCause") == Some("stream_suspended")
            || line.contains("went to sleep mid-response");
        if !(errored && cut) {
            return None;
        }
        return str_of(&obj, "uuid").map(String::from);
    }
    None
}

/// True once the transcript holds a result for the latest call of `tool` with this input, meaning it
/// was answered somewhere else (the terminal) while a hook still held it open.
pub fn tool_finished(tool: &str, input: &Value, path: &str) -> bool {
    let wanted = json_sorted(input, false);
    let Ok(lines) = read_lines(path, 500_000) else {
        return false;
    };
    let mut call: Option<String> = None;
    let mut finished = HashSet::new();
    for line in lines.iter().filter(|l| l.len() <= MAX_LINE_BYTES) {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let obj = Value::Object(map);
        let Some(content) = obj
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for item in content {
            match str_of(item, "type") {
                Some("tool_use")
                    if str_of(item, "name") == Some(tool)
                        && item.get("input").map(|i| json_sorted(i, false)).as_deref()
                            == Some(&wanted) =>
                {
                    call = str_of(item, "id").map(String::from);
                }
                Some("tool_result") => {
                    if let Some(id) = str_of(item, "tool_use_id") {
                        finished.insert(id.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    call.is_some_and(|id| finished.contains(&id))
}
