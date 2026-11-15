use super::message::{cut_chips, Message};
use super::restart::{carries_result, flag, WINDOW_BYTES};
use super::tail::read_lines;
use super::text::between;
use super::typed::typed_prompt;
use super::util::{parse_object, str_of};
use crate::sessions::iso::parse_iso_ms;
use serde_json::Value;

const EDGE_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seen {
    pub enqueued: usize,
    pub taken: usize,
}

pub(super) fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn said(obj: &Value) -> String {
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

#[derive(Default)]
pub(super) struct Queue {
    pub(super) waiting: Vec<String>,
    pub(super) batch: Vec<String>,
}

impl Queue {
    pub(super) fn operation(&mut self, obj: &Value) {
        let content = str_of(obj, "content");
        match str_of(obj, "operation") {
            Some("enqueue") => self.waiting.extend(content.map(String::from)),
            Some("dequeue") if !self.waiting.is_empty() => {
                self.batch.push(self.waiting.remove(0));
            }
            Some("popAll") => {
                self.waiting.clear();
                self.batch.clear();
            }
            Some("remove") => self.leave(content),
            _ => {}
        }
    }

    fn leave(&mut self, content: Option<&str>) {
        let Some(content) = content.map(squeezed) else {
            return;
        };
        if let Some(at) = self.waiting.iter().position(|e| squeezed(e) == content) {
            self.waiting.remove(at);
        }
    }
}

fn wants_something(message: &Message) -> bool {
    !message.text.is_empty() || message.pictures > 0
}

pub(super) fn entry_matches(message: &Message, content: &str) -> bool {
    if !wants_something(message) {
        return false;
    }
    let text = squeezed(&cut_chips(content));
    text == message.text || message.bash.as_deref() == Some(text.as_str())
}

fn line_matches(message: &Message, text: &str, images: usize) -> bool {
    if !wants_something(message) {
        return false;
    }
    if let (Some(bash), Some(inner)) = (
        &message.bash,
        between(text, "<bash-input>", "</bash-input>", true),
    ) {
        if squeezed(&inner) == *bash {
            return true;
        }
    }
    squeezed(&cut_chips(text)) == message.text && images >= message.pictures
}

fn command_matches(message: &Message, text: &str) -> bool {
    let Some((name, args)) = &message.command else {
        return false;
    };
    let Some(named) = between(text, "<command-name>", "</command-name>", true) else {
        return false;
    };
    let given = between(text, "<command-args>", "</command-args>", true).unwrap_or_default();
    named.trim_start_matches('/') == name && squeezed(&given) == *args
}

fn took_prompt(obj: &Value, text: &str, images: usize) -> bool {
    typed_prompt(obj, text) || (text.trim().is_empty() && images > 0 && !carries_result(obj))
}

fn takes(message: &Message, queue: &mut Queue, text: &str, images: usize) -> usize {
    let batch = std::mem::take(&mut queue.batch);
    if batch.len() >= 2 && squeezed(text) == squeezed(&batch.join("\n")) {
        if images < message.pictures {
            return 0;
        }
        return batch.iter().filter(|e| entry_matches(message, e)).count();
    }
    usize::from(line_matches(message, text, images))
}

fn image_blocks(content: Option<&Value>) -> usize {
    content.and_then(Value::as_array).map_or(0, |blocks| {
        blocks
            .iter()
            .filter(|b| str_of(b, "type") == Some("image"))
            .count()
    })
}

pub(super) fn attachment_text(obj: &Value) -> Option<(String, usize)> {
    let attachment = obj.get("attachment")?;
    if str_of(attachment, "type") != Some("queued_command") {
        return None;
    }
    let prompt = attachment.get("prompt");
    let text = match prompt {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| str_of(b, "type") == Some("text"))
            .filter_map(|b| str_of(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    Some((text, image_blocks(prompt)))
}

pub fn count_since(path: &str, message: &Message, since_ms: i64) -> Option<Seen> {
    let size = std::fs::metadata(path).ok()?.len();
    let lines = read_lines(path, WINDOW_BYTES).ok()?;
    let mut reaches_back = size <= WINDOW_BYTES;
    let mut seen = Seen::default();
    let mut queue = Queue::default();
    for line in &lines {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let obj = Value::Object(map);
        let Some(at) = str_of(&obj, "timestamp").and_then(parse_iso_ms) else {
            continue;
        };
        let counting = at >= since_ms;
        if !counting {
            reaches_back = true;
        }
        if flag(&obj, "isSidechain") || flag(&obj, "isMeta") || flag(&obj, "isCompactSummary") {
            continue;
        }
        match str_of(&obj, "type") {
            Some("queue-operation") => {
                queue.operation(&obj);
                if counting
                    && str_of(&obj, "operation") == Some("enqueue")
                    && str_of(&obj, "content").is_some_and(|c| entry_matches(message, c))
                {
                    seen.enqueued += 1;
                }
            }
            Some("user") if !flag(&obj, "isSidechain") && !carries_result(&obj) => {
                let text = said(&obj);
                let images = image_blocks(obj.pointer("/message/content"));
                if took_prompt(&obj, &text, images) {
                    let taken = takes(message, &mut queue, &text, images);
                    if counting {
                        seen.taken += taken;
                    }
                } else {
                    queue.batch.clear();
                    if counting && command_matches(message, &text) {
                        seen.taken += 1;
                    }
                }
            }
            Some("system") if str_of(&obj, "subtype") == Some("local_command") => {
                let text = str_of(&obj, "content").unwrap_or_default();
                queue.batch.clear();
                if counting && command_matches(message, text) {
                    seen.taken += 1;
                }
            }
            Some("attachment") => {
                let Some((text, images)) = attachment_text(&obj) else {
                    continue;
                };
                queue.leave(Some(&text));
                if took_prompt(&obj, &text, images) {
                    let taken = takes(message, &mut queue, &text, images);
                    if counting {
                        seen.taken += taken;
                    }
                } else {
                    queue.batch.clear();
                }
            }
            _ => {}
        }
    }
    reaches_back.then_some(seen)
}

#[cfg(test)]
#[path = "sent_count_tests.rs"]
mod count_tests;
#[cfg(test)]
#[path = "sent_tests.rs"]
mod tests;
