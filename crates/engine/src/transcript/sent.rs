use super::restart::{carries_result, flag, WINDOW_BYTES};
use super::tail::read_lines;
use super::util::{parse_object, str_of};
use crate::sessions::iso::parse_iso_ms;
use serde_json::Value;

const EDGE_CHARS: usize = 40;

fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn said(obj: &Value) -> String {
    match obj.pointer("/message/content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| str_of(b, "type") == Some("text"))
            .filter_map(|b| str_of(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn typed_text(obj: &Value) -> Option<String> {
    match str_of(obj, "type") {
        Some("user") if !flag(obj, "isSidechain") && !carries_result(obj) => Some(said(obj)),
        Some("queue-operation") if str_of(obj, "operation") == Some("enqueue") => {
            str_of(obj, "content").map(String::from)
        }
        _ => None,
    }
}

fn marks(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= EDGE_CHARS * 2 {
        return vec![text.to_owned()];
    }
    let edge = |part: &[char]| part.iter().collect::<String>();
    vec![
        edge(&chars[..EDGE_CHARS]),
        edge(&chars[chars.len() - EDGE_CHARS..]),
    ]
}

pub fn sent_since(path: &str, text: &str, since_ms: i64) -> Option<bool> {
    let wanted = squeezed(text);
    if wanted.is_empty() {
        return Some(false);
    }
    let size = std::fs::metadata(path).ok()?.len();
    let lines = read_lines(path, WINDOW_BYTES).ok()?;
    let marks = marks(&wanted);
    let mut reaches_back = size <= WINDOW_BYTES;
    for line in &lines {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let obj = Value::Object(map);
        let Some(at) = str_of(&obj, "timestamp").and_then(parse_iso_ms) else {
            continue;
        };
        if at < since_ms {
            reaches_back = true;
            continue;
        }
        let Some(typed) = typed_text(&obj) else {
            continue;
        };
        let typed = squeezed(&typed);
        if marks.iter().any(|mark| typed.contains(mark.as_str())) {
            return Some(true);
        }
    }
    reaches_back.then_some(false)
}

#[cfg(test)]
#[path = "sent_tests.rs"]
mod tests;
