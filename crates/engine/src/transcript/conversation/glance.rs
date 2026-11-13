use super::parser::{Parser, MAX_LINE};
use crate::transcript::glance::{has, has_step, peek as peek_with, widen, Peeked, TailParser};
use crate::transcript::model::Conversation;
use crate::transcript::util::{count_of, parse_object, str_of};
use serde_json::Value;

fn held(line: &[u8]) -> Option<u64> {
    if line.len() > MAX_LINE || !(has(line, b"\"usage\"") || has(line, b"compact_boundary")) {
        return None;
    }
    let obj = Value::Object(parse_object(line)?);
    match str_of(&obj, "type") {
        Some("assistant") => {
            let message = obj.get("message")?;
            message.get("content").and_then(Value::as_array)?;
            let usage = message.get("usage").filter(|u| u.is_object())?;
            let n = |k: &str| usage.get(k).map_or(0, count_of);
            let total = n("input_tokens")
                .saturating_add(n("cache_creation_input_tokens"))
                .saturating_add(n("cache_read_input_tokens"))
                .saturating_add(n("output_tokens"));
            (total > 0).then_some(total)
        }
        Some("system") if str_of(&obj, "subtype") == Some("compact_boundary") => obj
            .get("compactMetadata")
            .and_then(|m| m.get("postTokens"))
            .map(count_of),
        _ => None,
    }
}

pub fn context_tokens(path: &str) -> Option<u64> {
    widen(path, |window| {
        let found = window.lines().into_iter().rev().find_map(held);
        (found, found.is_some())
    })
    .flatten()
}

fn tail(lines: &[&[u8]]) -> Conversation {
    let mut parser = Parser::for_subagent();
    for line in lines {
        parser.consume(line);
    }
    parser.conversation()
}

pub fn recent(path: &str) -> Option<Conversation> {
    widen(path, |window| {
        let conversation = tail(&window.lines());
        let done = has_step(&conversation);
        (conversation, done)
    })
}

pub fn peek(path: &str, with_recent: bool) -> Option<Peeked> {
    peek_with(path, held, with_recent.then_some(tail as TailParser))
}
