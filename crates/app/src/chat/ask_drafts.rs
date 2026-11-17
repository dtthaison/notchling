use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::acting::{AskAnswer, Request};
use crate::chat::asks::{lost_line, AskRow};
use crate::chat::prompt_flow::failure_text;
use crate::notch::answer::{type_key, Typed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Choose(usize),
    Send,
    Dismiss,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Keyed {
    Ignored,
    Changed,
    Send(Option<Request>),
}

#[derive(Default)]
pub struct Drafts {
    typed: HashMap<String, String>,
    lost: HashMap<String, String>,
    busy: HashSet<String>,
}

impl Drafts {
    pub fn typed(&self, id: &str) -> &str {
        self.typed.get(id).map(String::as_str).unwrap_or("")
    }

    pub fn busy(&self, id: &str) -> bool {
        self.busy.contains(id)
    }

    pub fn lost(&self, id: &str) -> Option<&str> {
        self.lost.get(id).map(String::as_str)
    }

    pub fn act(&mut self, row: &AskRow, act: Act) -> Option<Request> {
        if !row.is_open() || self.busy(&row.id) {
            return None;
        }
        let request = match act {
            Act::Choose(n) => {
                let n = u8::try_from(n).ok().filter(|n| *n >= 1)?;
                if usize::from(n) > row.choices.len() {
                    return None;
                }
                Request::ask_answer(&row.id, AskAnswer::Choice(n))
            }
            Act::Send => {
                let text = self.typed(&row.id).trim().to_owned();
                if text.is_empty() {
                    return None;
                }
                self.typed.remove(&row.id);
                Request::ask_answer(&row.id, AskAnswer::Text(&text))
            }
            Act::Dismiss => Request::ask_dismiss(&row.id),
        };
        self.busy.insert(row.id.clone());
        self.lost.remove(&row.id);
        Some(request)
    }

    pub fn key(&mut self, row: &AskRow, key: &str, ch: Option<&str>, modified: bool) -> Keyed {
        if !row.is_open() {
            return Keyed::Ignored;
        }
        let typed = self.typed.entry(row.id.clone()).or_default();
        let result = match type_key(typed, key, ch, modified) {
            Typed::Submit => return Keyed::Send(self.act(row, Act::Send)),
            Typed::Changed => Keyed::Changed,
            Typed::Ignored => Keyed::Ignored,
        };
        if self.typed(&row.id).is_empty() {
            self.typed.remove(&row.id);
        }
        result
    }

    pub fn settle(&mut self, id: &str, reply: Result<Value, String>) {
        self.busy.remove(id);
        match reply {
            Ok(value) => {
                let taken = ["answered", "dismissed"]
                    .iter()
                    .any(|key| value.get(key).and_then(Value::as_bool) == Some(true));
                match (taken, lost_line(&value)) {
                    (false, Some(line)) => {
                        self.lost.insert(id.to_owned(), line);
                    }
                    _ => {
                        self.lost.remove(id);
                    }
                }
            }
            Err(message) => {
                self.lost.insert(id.to_owned(), failure_text(&message));
            }
        }
    }

    pub fn keep_only(&mut self, ids: &[&str]) {
        let keep = |id: &String| ids.contains(&id.as_str());
        self.typed.retain(|id, _| keep(id));
        self.lost.retain(|id, _| keep(id));
        self.busy.retain(keep);
    }
}

#[cfg(test)]
#[path = "ask_drafts_tests.rs"]
mod tests;
