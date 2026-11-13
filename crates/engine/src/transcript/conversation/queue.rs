use crate::transcript::typed::typed_prompt;
use crate::transcript::util::str_of;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Queue {
    items: Vec<String>,
    dequeued: Option<(usize, String)>,
}

impl Queue {
    pub(super) fn items(&self) -> &[String] {
        &self.items
    }

    pub(super) fn operation(&mut self, obj: &Value) {
        let content = str_of(obj, "content").unwrap_or("").to_string();
        self.dequeued = None;
        match str_of(obj, "operation") {
            Some("enqueue") => self.items.push(content),
            Some("dequeue") => {
                if !self.items.is_empty() {
                    self.dequeued = Some((0, self.items.remove(0)));
                }
            }
            Some("remove" | "popAll") => {
                if let Some(i) = self.items.iter().position(|q| *q == content) {
                    self.items.remove(i);
                }
            }
            _ => {}
        }
    }

    pub(super) fn user_line(&mut self, line: &Value) {
        let Some(message) = line.get("message") else {
            return;
        };
        let Some(text) = prompt_text(message) else {
            return;
        };
        let Some((at, guessed)) = self.dequeued.take() else {
            if typed_prompt(line, &text) {
                self.items.clear();
            }
            return;
        };
        if names(&guessed, &text) {
            return;
        }
        let Some(taken) = self.items.iter().position(|q| names(q, &text)) else {
            self.dequeued = Some((at, guessed));
            return;
        };
        self.items.remove(taken);
        self.items.insert(at.min(self.items.len()), guessed);
    }
}

fn names(entry: &str, line: &str) -> bool {
    entry == line || bash_command(line).is_some_and(|command| entry.trim() == command)
}

fn bash_command(line: &str) -> Option<&str> {
    let inner = line
        .trim()
        .strip_prefix("<bash-input>")?
        .strip_suffix("</bash-input>")?;
    Some(inner.trim())
}

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
