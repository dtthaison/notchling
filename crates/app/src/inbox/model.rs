use notchling_engine::inbox::{ActionId, Item, State};
use notchling_local_api::topics;
use serde_json::{json, Value};

use crate::chat::format::age;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub ago: String,
    pub actions: Vec<(ActionId, String, bool)>,
    pub enabled: bool,
    pub open: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboxModel {
    items: Vec<Item>,
    now_ms: i64,
}

fn item_of(value: &Value) -> Option<Item> {
    serde_json::from_value(value.clone()).ok()
}

fn row_of(item: &Item, now_ms: i64) -> Row {
    let suffix = match item.state {
        State::Expired => Some("Expired"),
        State::Withdrawn => Some("Withdrawn"),
        State::Open | State::Answered => None,
    };
    let detail = match suffix {
        Some(word) if item.detail.is_empty() => word.to_owned(),
        Some(word) => format!("{} · {word}", item.detail),
        None => item.detail.clone(),
    };
    Row {
        id: item.id.clone(),
        title: item.title.clone(),
        detail,
        ago: age(now_ms, item.created_ms),
        actions: item
            .actions
            .iter()
            .map(|a| (a.id, a.label.clone(), a.primary))
            .collect(),
        enabled: item.state == State::Open,
        open: item.open.clone(),
    }
}

impl InboxModel {
    pub fn from_items(items: Vec<Item>, now_ms: i64) -> InboxModel {
        InboxModel { items, now_ms }
    }

    pub fn load(&mut self, reply: &Value) {
        let Some(items) = reply.get("items").and_then(Value::as_array) else {
            return;
        };
        self.items = items.iter().filter_map(item_of).collect();
    }

    pub fn apply(&mut self, event: &Value) {
        let name = event.get("event").and_then(Value::as_str);
        let data = event.get("data");
        match (name, data) {
            (Some(topics::INBOX_CHANGED), Some(data)) => {
                let Some(item) = data.get("item").and_then(item_of) else {
                    return;
                };
                match self.items.iter_mut().find(|held| held.id == item.id) {
                    Some(held) => *held = item,
                    None => self.items.push(item),
                }
            }
            (Some(topics::INBOX_REMOVED), Some(data)) => {
                if let Some(id) = data.get("id").and_then(Value::as_str) {
                    self.items.retain(|held| held.id != id);
                }
            }
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn set_now(&mut self, now_ms: i64) {
        self.now_ms = now_ms;
    }

    pub fn sections(&self) -> (Vec<Row>, Vec<Row>) {
        let mut needs: Vec<&Item> = Vec::new();
        let mut done: Vec<&Item> = Vec::new();
        for item in &self.items {
            if item.state == State::Open {
                needs.push(item);
            } else {
                done.push(item);
            }
        }
        needs.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.id.cmp(&b.id)));
        let closed = |item: &Item| item.closed_ms.unwrap_or(item.created_ms);
        done.sort_by(|a, b| closed(b).cmp(&closed(a)).then(a.id.cmp(&b.id)));
        let rows = |items: Vec<&Item>| {
            items
                .into_iter()
                .map(|item| row_of(item, self.now_ms))
                .collect()
        };
        (rows(needs), rows(done))
    }

    pub fn unread(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == State::Open && !item.seen)
            .count()
    }

    pub fn unseen_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.state == State::Open && !item.seen)
            .map(|item| item.id.clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Approval(String),
    Request(String),
}

pub fn target(open: &Value) -> Option<Target> {
    if let Some(ticket) = open.get("vault_approval").and_then(Value::as_str) {
        return Some(Target::Approval(ticket.to_owned()));
    }
    let ticket = open.get("vault_request").and_then(Value::as_str)?;
    Some(Target::Request(ticket.to_owned()))
}

pub fn is_local(action: ActionId) -> bool {
    action == ActionId::Open
}

pub fn answer_args(id: &str, action: ActionId) -> Value {
    json!({"id": id, "action": action})
}

pub fn seen_args(ids: &[String]) -> Value {
    json!({"ids": ids})
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
