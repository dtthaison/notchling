use super::message::{cut_chips, Message};
use super::restart::{carries_result, flag, WINDOW_BYTES};
use super::sent::{attachment_text, entry_matches, said, squeezed, Queue};
use super::tail::read_lines;
use super::text::between;
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

fn names(entry: &str, line: &str) -> bool {
    let bash = between(line, "<bash-input>", "</bash-input>", true);
    squeezed(&cut_chips(entry)) == squeezed(&cut_chips(line))
        || bash.is_some_and(|command| squeezed(&command) == squeezed(entry))
}

fn prompt_like(entry: &str) -> bool {
    typed_prompt(&Value::Null, entry)
}

fn user_line(queue: &mut Queue, obj: &Value) {
    let text = said(obj);
    if text.trim().is_empty() {
        return;
    }
    let batch = std::mem::take(&mut queue.batch);
    if batch.is_empty() {
        if typed_prompt(obj, &text) {
            queue.waiting.clear();
        }
        return;
    }
    if squeezed(&text) == squeezed(&batch.join("\n")) || batch.len() != 1 {
        return;
    }
    if names(&batch[0], &text) {
        return;
    }
    if let Some(at) = queue.waiting.iter().position(|e| names(e, &text)) {
        queue.waiting.remove(at);
        queue.waiting.insert(0, batch[0].clone());
    }
}

fn leave(queue: &mut Queue, text: &str) {
    if let Some(at) = queue.waiting.iter().position(|e| names(e, text)) {
        queue.waiting.remove(at);
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
        match str_of(&obj, "type") {
            Some("queue-operation") => {
                if is_known_operation(&obj) {
                    queue.operation(&obj);
                } else {
                    unknown += 1;
                }
            }
            Some("user") if !flag(&obj, "isSidechain") && !carries_result(&obj) => {
                user_line(&mut queue, &obj);
            }
            Some("system")
                if str_of(&obj, "subtype") == Some("turn_duration")
                    && !flag(&obj, "isSidechain") =>
            {
                queue.waiting.clear();
            }
            Some("attachment")
                if obj.pointer("/attachment/type").and_then(Value::as_str)
                    == Some("queued_command") =>
            {
                match attachment_text(&obj) {
                    Some((text, _)) => leave(&mut queue, &text),
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
