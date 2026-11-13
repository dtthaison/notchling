use super::Parser;
use crate::transcript::glance::{has, has_step, widen};
use crate::transcript::model::Conversation;
use crate::transcript::util::{count_of, parse_object, str_of};
use serde_json::Value;

fn held(line: &[u8]) -> Option<u64> {
    if !has(line, b"token_count") {
        return None;
    }
    let obj = Value::Object(parse_object(line)?);
    let payload = obj.get("payload").filter(|p| p.is_object())?;
    if str_of(&obj, "type") != Some("event_msg") || str_of(payload, "type") != Some("token_count") {
        return None;
    }
    let last = payload
        .get("info")
        .filter(|i| i.is_object())?
        .get("last_token_usage")
        .filter(|t| t.is_object())?;
    let n = |k: &str| last.get(k).map_or(0, count_of);
    Some(n("input_tokens").saturating_add(n("output_tokens")))
}

pub fn context_tokens(path: &str) -> Option<u64> {
    widen(path, |window| {
        let found = window.lines().into_iter().rev().find_map(held);
        (found, found.is_some())
    })
    .flatten()
}

pub fn recent(path: &str) -> Option<Conversation> {
    widen(path, |window| {
        let mut parser = Parser::new();
        for line in window.lines() {
            parser.consume(line);
        }
        let conversation = parser.conversation();
        let done = has_step(&conversation);
        (conversation, done)
    })
}
