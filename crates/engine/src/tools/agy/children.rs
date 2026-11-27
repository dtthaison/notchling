use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::sessions::iso::parse_iso_ms;
use crate::transcript::glance::{Window, FIRST};
use crate::transcript::model::Timestamp;

const PARENT_WINDOW: u64 = FIRST * 4;
const HEAD: u64 = 8 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub sent: bool,
    pub failed: bool,
}

fn steps(path: &Path, len: u64) -> Vec<Value> {
    let Ok(window) = Window::read(&path.to_string_lossy(), len) else {
        return Vec::new();
    };
    window
        .lines()
        .iter()
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .collect()
}

fn type_of(step: &Value) -> &str {
    step.get("type").and_then(Value::as_str).unwrap_or("")
}

fn calls(step: &Value) -> impl Iterator<Item = &Value> {
    step.get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn named<'a>(step: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> {
    calls(step).filter(move |c| c.get("name").and_then(Value::as_str) == Some(name))
}

pub fn signals(log: &Path) -> Signals {
    let steps = steps(log, FIRST);
    Signals {
        sent: steps
            .iter()
            .any(|s| type_of(s) == "PLANNER_RESPONSE" && named(s, "send_message").next().is_some()),
        failed: steps.last().is_some_and(|s| type_of(s) == "ERROR_MESSAGE"),
    }
}

fn senders(content: &str) -> Vec<String> {
    if !content.contains("[Message]") {
        return Vec::new();
    }
    content
        .split("sender=")
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
        })
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn heard_from(parent_log: &Path) -> HashSet<String> {
    steps(parent_log, PARENT_WINDOW)
        .iter()
        .filter(|s| type_of(s) == "SYSTEM_MESSAGE")
        .filter_map(|s| s.get("content").and_then(Value::as_str))
        .flat_map(senders)
        .collect()
}

fn unquoted(value: Option<&Value>) -> String {
    let text = match value {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(Value::String(inner)) => inner,
            _ => text.clone(),
        },
        _ => String::new(),
    };
    text.lines().next().unwrap_or("").trim().to_owned()
}

pub fn roles(parent_log: &Path) -> Vec<String> {
    steps(parent_log, PARENT_WINDOW)
        .iter()
        .filter(|s| type_of(s) == "PLANNER_RESPONSE")
        .flat_map(|s| {
            named(s, "invoke_subagent")
                .map(|c| unquoted(c.get("args").and_then(|a| a.get("role"))))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn started(log: &Path) -> Option<Timestamp> {
    let file = std::fs::File::open(log).ok()?;
    let mut head = Vec::new();
    file.take(HEAD).read_to_end(&mut head).ok()?;
    let first = head.split(|&b| b == b'\n').next()?;
    let step: Value = serde_json::from_slice(first).ok()?;
    let at = step.get("created_at").and_then(Value::as_str)?;
    parse_iso_ms(at).map(|ms| ms as f64 / 1000.0)
}

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;
