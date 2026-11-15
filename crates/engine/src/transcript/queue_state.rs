use super::message::Message;
use super::restart::{carries_result, flag, WINDOW_BYTES};
use super::sent::{
    attachment_text, entry_matches, image_blocks, names, said, squeezed, took_prompt, Queue,
};
use super::tail::read_lines;
use super::typed::typed_prompt;
use super::util::{parse_object, str_of};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueState {
    pub waiting: Vec<String>,
    pub unknown: u32,
}

impl QueueState {
    pub fn listed(&self, message: &Message) -> usize {
        self.waiting
            .iter()
            .filter(|entry| entry_matches(message, entry))
            .count()
    }
}

fn prompt_like(entry: &str) -> bool {
    typed_prompt(&Value::Null, entry)
}

fn user_line(queue: &mut Queue, obj: &Value) {
    let text = said(obj);
    let images = image_blocks(obj.pointer("/message/content"));
    if text.trim().is_empty() && images == 0 {
        return;
    }
    if !took_prompt(obj, &text, images) {
        queue.other_line(&text);
        return;
    }
    let mut batch = std::mem::take(&mut queue.batch);
    if batch.len() > 1 {
        if squeezed(&text) == squeezed(&batch.join("\n")) {
            return;
        }
        batch.clear();
    }
    let listed = queue.waiting.iter().position(|e| names(e, &text));
    let Some(first) = batch.first() else {
        queue.forget_waiting(listed);
        return;
    };
    if names(first, &text) {
        return;
    }
    match listed {
        Some(at) => {
            queue.waiting.remove(at);
            queue.waiting.insert(0, first.clone());
        }
        None => queue.forget_waiting(None),
    }
}

fn is_known_operation(obj: &Value) -> bool {
    matches!(
        str_of(obj, "operation"),
        Some("enqueue" | "dequeue" | "remove" | "popAll")
    )
}

pub fn queue_state(path: &str) -> Option<QueueState> {
    let lines = read_lines(path, WINDOW_BYTES).ok()?;
    let mut queue = Queue::default();
    let mut unknown = 0;
    for line in &lines {
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let obj = Value::Object(map);
        let set_aside =
            flag(&obj, "isSidechain") || flag(&obj, "isMeta") || flag(&obj, "isCompactSummary");
        if set_aside {
            continue;
        }
        match str_of(&obj, "type") {
            Some("queue-operation") => {
                if is_known_operation(&obj) {
                    queue.operation(&obj);
                } else {
                    unknown += 1;
                }
            }
            Some("user") if carries_result(&obj) => queue.delivered_in_result(&obj),
            Some("user") => user_line(&mut queue, &obj),
            Some("system") if str_of(&obj, "subtype") == Some("turn_duration") => {
                queue.turn_end();
            }
            Some("system") if str_of(&obj, "subtype") == Some("local_command") => {
                queue.other_line(str_of(&obj, "content").unwrap_or_default());
            }
            Some("attachment")
                if obj.pointer("/attachment/type").and_then(Value::as_str)
                    == Some("queued_command") =>
            {
                match attachment_text(&obj) {
                    Some((text, _)) => queue.taken_in_turn(&text),
                    None => unknown += 1,
                }
            }
            _ => {}
        }
    }
    let waiting = queue
        .waiting
        .into_iter()
        .filter(|entry| prompt_like(entry))
        .collect();
    Some(QueueState { waiting, unknown })
}

#[cfg(test)]
#[path = "queue_state_tests.rs"]
mod tests;
