//! Claude Code's input queue as its `queue-operation` lines leave it: what you sent while it
//! worked (and the task notifications it queues the same way), not yet taken.
//!
//! A `dequeue` doesn't say which entry it took, and Claude Code 2.1.322 doesn't always take the
//! oldest: a subagent's notification goes before older shell ones, which it then removes. So a
//! dequeue is first taken to be the oldest, and the user line that follows, which carries the
//! entry's text, corrects that. Read strictly oldest-first, one stale entry stayed, every later
//! dequeue took the wrong one, and the next prompt you sent stayed "Queued" after it was answered.

use crate::transcript::util::str_of;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Queue {
    /// Oldest first.
    items: Vec<String>,
    /// What the last dequeue was taken to be, and where it sat; any other queue line lets the
    /// guess stand.
    dequeued: Option<(usize, String)>,
}

impl Queue {
    pub(super) fn items(&self) -> &[String] {
        &self.items
    }

    /// A `queue-operation` line.
    pub(super) fn operation(&mut self, obj: &Value) {
        let content = str_of(obj, "content").unwrap_or("").to_string();
        self.dequeued = None;
        match str_of(obj, "operation") {
            Some("enqueue") => self.items.push(content),
            // The oldest, until the user line after it says which it was.
            Some("dequeue") => {
                if !self.items.is_empty() {
                    self.dequeued = Some((0, self.items.remove(0)));
                }
            }
            // popAll: Up in the input box took the queued messages back to edit (one line each).
            Some("remove" | "popAll") => {
                if let Some(i) = self.items.iter().position(|q| *q == content) {
                    self.items.remove(i);
                }
            }
            _ => {}
        }
    }

    /// A user line's `message`: if the last dequeue was taken to be another entry and this line's
    /// text is still queued, it was this one that went, and the other is put back.
    pub(super) fn user_line(&mut self, message: &Value) {
        let Some(text) = prompt_text(message) else {
            return;
        };
        let Some((at, guessed)) = self.dequeued.take() else {
            return;
        };
        if guessed == text {
            return;
        }
        let Some(taken) = self.items.iter().position(|q| *q == text) else {
            // Not a queued prompt (a meta line): the next user line may be.
            self.dequeued = Some((at, guessed));
            return;
        };
        self.items.remove(taken);
        self.items.insert(at.min(self.items.len()), guessed);
    }
}

/// A user line's prompt as its queue entry was written: the string, or the text blocks joined.
/// `None` for a line with no text (a tool result).
fn prompt_text(message: &Value) -> Option<String> {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return Some(text.to_owned());
    }
    let texts: Vec<&str> = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|b| str_of(b, "type") == Some("text"))
        .filter_map(|b| str_of(b, "text"))
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}
