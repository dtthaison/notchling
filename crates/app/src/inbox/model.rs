use notchling_engine::inbox::{ActionId, Item, Kind, State};
use notchling_local_api::{ops, topics};
use notchling_locale as t;
use serde_json::{json, Value};

use crate::chat::format::age;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub result: Option<String>,
    pub ago: String,
    pub actions: Vec<(ActionId, String, bool)>,
    pub enabled: bool,
    pub open: Option<Value>,
    pub node: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboxModel {
    items: Vec<Item>,
    consents: Vec<Value>,
    now_ms: i64,
}

fn item_of(value: &Value) -> Option<Item> {
    serde_json::from_value(value.clone()).ok()
}

fn decided(answer: Option<ActionId>) -> &'static str {
    match answer {
        Some(ActionId::Allow) => "Allowed",
        Some(ActionId::AllowOnce) => "Allowed Once",
        Some(ActionId::AllowAlways) => "Always Allowed",
        Some(ActionId::Deny) => "Denied",
        Some(ActionId::Continue) => "Continued",
        Some(ActionId::Cancel) => "Cancelled",
        Some(ActionId::Done) => "Done",
        Some(ActionId::Open) => "Saved",
        Some(ActionId::Send) => "Sent",
        Some(ActionId::Discard) => "Discarded",
        Some(ActionId::MoveHere | ActionId::MoveAnyway) => "Moved",
        Some(ActionId::TakeBack) => "Taken Back",
        Some(ActionId::Keep) => "Kept",
        Some(ActionId::ContinueHere) => "Continued Here",
        Some(ActionId::WaitFor) => "Waiting",
        Some(ActionId::Stop) => "Stopped",
        Some(ActionId::KeepWaiting) => "Kept Waiting",
        Some(
            ActionId::Choice1
            | ActionId::Choice2
            | ActionId::Choice3
            | ActionId::Choice4
            | ActionId::Choice5
            | ActionId::Choice6,
        ) => "Answered",
        None => "Answered",
    }
}

fn place(by: &str) -> String {
    match by {
        "mac" => "Mac".to_owned(),
        "phone" => "Phone".to_owned(),
        "main" => t::inbox::question_by_main().to_owned(),
        other => other.to_owned(),
    }
}

fn question_words(item: &Item) -> String {
    let by = item
        .answered_by
        .as_deref()
        .filter(|by| !by.is_empty())
        .map_or_else(|| place("mac"), place);
    match item.answer {
        Some(ActionId::Cancel) => t::inbox::question_dismissed().to_owned(),
        Some(
            id @ (ActionId::Choice1
            | ActionId::Choice2
            | ActionId::Choice3
            | ActionId::Choice4
            | ActionId::Choice5
            | ActionId::Choice6),
        ) => match item.actions.iter().find(|a| a.id == id) {
            Some(action) => t::inbox::question_answered_with(&by, &action.label),
            None => t::inbox::question_answered_in_chat().to_owned(),
        },
        _ => t::inbox::question_answered_in_chat().to_owned(),
    }
}

fn result_of(item: &Item) -> Option<String> {
    if item.kind == Kind::Question && item.state == State::Answered {
        return Some(question_words(item));
    }
    match item.state {
        State::Open => None,
        State::Expired => Some("Expired".to_owned()),
        State::Withdrawn => Some("Withdrawn".to_owned()),
        State::Answered => {
            let word = decided(item.answer);
            Some(match item.answered_by.as_deref() {
                Some(by) if !by.is_empty() => format!("{word} · {}", place(by)),
                _ => word.to_owned(),
            })
        }
    }
}

const MACHINE_CHARS: usize = 8;

fn words_of(item: &Item) -> (String, String) {
    let trusted = notchling_engine::consent::worker_switch(item).and_then(|asked| {
        let (title, unlocks) = super::consent_card::switch_words(&asked.switch)?;
        let machine: String = asked.node.chars().take(MACHINE_CHARS).collect();
        Some((
            t::inbox::worker_switch_title(title, &machine),
            unlocks.to_owned(),
        ))
    });
    trusted.unwrap_or_else(|| (item.title.clone(), item.detail.clone()))
}

fn row_of(item: &Item, now_ms: i64) -> Row {
    let result = result_of(item);
    let (title, said) = words_of(item);
    let detail = match &result {
        Some(word) if said.is_empty() => word.clone(),
        Some(word) => format!("{said} · {word}"),
        None => said,
    };
    Row {
        id: item.id.clone(),
        title,
        detail,
        result,
        ago: age(now_ms, item.created_ms),
        actions: item
            .actions
            .iter()
            .map(|a| (a.id, a.label.clone(), a.primary))
            .collect(),
        enabled: item.state == State::Open,
        open: item.open.clone(),
        node: item.source.node.clone(),
    }
}

impl InboxModel {
    pub fn from_items(items: Vec<Item>, now_ms: i64) -> InboxModel {
        InboxModel {
            items,
            consents: Vec::new(),
            now_ms,
        }
    }

    pub fn load(&mut self, reply: &Value) {
        let Some(items) = reply.get("items").and_then(Value::as_array) else {
            return;
        };
        self.items = items.iter().filter_map(item_of).collect();
    }

    pub fn load_consents(&mut self, reply: &Value) {
        if let Some(rows) = reply.get("requests").and_then(Value::as_array) {
            self.consents = rows.clone();
        }
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn consent(&self, id: &str) -> Option<&Value> {
        self.consents
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
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
        self.consents.clear();
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

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Approval(String),
    Request(String),
    Settings { page: String, tool: Option<String> },
    Machines(String),
    Ask { id: String, session: String },
}

pub fn target(open: &Value) -> Option<Target> {
    if let Some(ticket) = open.get("vault_approval").and_then(Value::as_str) {
        return Some(Target::Approval(ticket.to_owned()));
    }
    if let Some(ticket) = open.get("vault_request").and_then(Value::as_str) {
        return Some(Target::Request(ticket.to_owned()));
    }
    if let Some(machine) = open.get("machines").and_then(Value::as_str) {
        return Some(Target::Machines(machine.to_owned()));
    }
    if let Some(id) = open.get("ask").and_then(Value::as_str) {
        let session = open.get("session").and_then(Value::as_str)?;
        return Some(Target::Ask {
            id: id.to_owned(),
            session: session.to_owned(),
        });
    }
    let page = open.get("settings").and_then(Value::as_str)?;
    Some(Target::Settings {
        page: page.to_owned(),
        tool: open.get("tool").and_then(Value::as_str).map(str::to_owned),
    })
}

pub fn consent_of(open: &Value) -> Option<&str> {
    open.get("consent").and_then(Value::as_str)
}

pub fn is_local(action: ActionId) -> bool {
    action == ActionId::Open
}

pub fn answer_args(id: &str, action: ActionId) -> Value {
    json!({"id": id, "action": action})
}

pub fn consent_answer_args(id: &str, allow: bool) -> Value {
    json!({"consentId": id, "decision": if allow { "allow" } else { "deny" }})
}

pub fn switch_apply(id: &str, switch: &str) -> (&'static str, Value) {
    match switch {
        "acting" => (
            ops::SETTINGS_SET_ACTING,
            json!({"on": true, "consentId": id}),
        ),
        "bridge" => (ops::BRIDGE_SET, json!({"on": true, "consentId": id})),
        other => (
            ops::SETTINGS_SET_SWITCH,
            json!({"switch": other, "on": true, "consentId": id}),
        ),
    }
}

pub fn cluster_apply_args(id: &str, args: &Value) -> Value {
    let mut out = args.clone();
    if let Some(map) = out.as_object_mut() {
        map.insert("consentId".into(), Value::String(id.to_owned()));
    }
    out
}

pub fn seen_args(ids: &[String]) -> Value {
    json!({"ids": ids})
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
