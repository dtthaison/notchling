use std::collections::HashSet;

use notchling_engine::inbox::{ActionId, AlertAction, Item, Kind, OutcomeState, State};
use notchling_local_api::{ops, topics};
use notchling_locale as t;
use serde_json::{json, Value};

use super::triage::{section_of, Section};
use crate::chat::format::{age, duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Good,
    Bad,
    Warn,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub text: String,
    pub tone: Tone,
    pub drain: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeRoute {
    Consent(String),
    Approval(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub section: Section,
    pub kind: Kind,
    pub unseen: bool,
    pub blocking: bool,
    pub machine: Option<String>,
    pub session: Option<String>,
    pub waited: Option<String>,
    pub pill: Pill,
    pub change: Option<ChangeRoute>,
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
    pub(super) items: Vec<Item>,
    pub(super) consents: Vec<Value>,
    pub(super) now_ms: i64,
    pub(super) cleared: HashSet<String>,
}

fn item_of(value: &Value) -> Option<Item> {
    serde_json::from_value(value.clone()).ok()
}

const SEP: &str = " · ";

pub(super) fn decided(answer: Option<ActionId>) -> &'static str {
    match answer {
        Some(ActionId::Allow) => t::consent::decided_allowed(),
        Some(ActionId::AllowOnce) => t::consent::decided_allowed_once(),
        Some(ActionId::AllowAlways) => t::consent::decided_allowed_always(),
        Some(ActionId::Deny) => t::consent::decided_denied(),
        Some(ActionId::Continue) => t::inbox::decided_continued(),
        Some(ActionId::Cancel) => t::inbox::decided_cancelled(),
        Some(ActionId::Done) => t::inbox::decided_done(),
        Some(ActionId::Open) => t::inbox::decided_saved(),
        Some(ActionId::Send) => t::inbox::decided_sent(),
        Some(ActionId::Discard) => t::inbox::decided_discarded(),
        Some(ActionId::MoveHere | ActionId::MoveAnyway) => t::inbox::decided_moved(),
        Some(ActionId::TakeBack) => t::inbox::decided_taken_back(),
        Some(ActionId::Keep) => t::inbox::decided_kept(),
        Some(ActionId::ContinueHere) => t::inbox::decided_continued_here(),
        Some(ActionId::WaitFor) => t::inbox::decided_waiting(),
        Some(ActionId::Stop) => t::inbox::decided_stopped(),
        Some(ActionId::KeepWaiting) => t::inbox::decided_kept_waiting(),
        Some(
            ActionId::Choice1
            | ActionId::Choice2
            | ActionId::Choice3
            | ActionId::Choice4
            | ActionId::Choice5
            | ActionId::Choice6,
        ) => t::inbox::decided_answered(),
        None => t::inbox::decided_answered(),
    }
}

fn place(by: &str) -> String {
    match by {
        "mac" => t::inbox::by_mac().to_owned(),
        "phone" => t::inbox::by_phone().to_owned(),
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
        State::Expired => Some(t::inbox::decided_expired().to_owned()),
        State::Withdrawn => Some(t::inbox::decided_withdrawn().to_owned()),
        State::Answered => {
            let word = decided(item.answer);
            Some(match item.answered_by.as_deref() {
                Some(by) if !by.is_empty() => [word, &place(by)].join(SEP),
                _ => word.to_owned(),
            })
        }
    }
}

const MACHINE_CHARS: usize = 8;

pub(super) fn sent_back(item: &Item) -> Option<&str> {
    if item.state != State::Answered || item.answer != Some(ActionId::Deny) {
        return None;
    }
    item.note.as_deref().filter(|note| !note.trim().is_empty())
}

pub(super) fn change_route(item: &Item) -> Option<ChangeRoute> {
    if item.state != State::Open
        || super::triage::is_applying(item)
        || !item.alert_actions.contains(&AlertAction::Change)
    {
        return None;
    }
    if let Some(consent) = item.open.as_ref().and_then(consent_of) {
        return Some(ChangeRoute::Consent(consent.to_owned()));
    }
    let ticket = item.open.as_ref()?.get("vault_approval")?.as_str()?;
    Some(ChangeRoute::Approval(ticket.to_owned()))
}

fn pill_of(item: &Item, result: Option<&str>, now_ms: i64) -> Pill {
    let plain = |text: String, tone: Tone| Pill {
        text,
        tone,
        drain: None,
    };
    if let Some(outcome) = &item.outcome {
        return match outcome.state {
            OutcomeState::Applying => plain(t::inbox::pill_applying().to_owned(), Tone::Plain),
            OutcomeState::Applied => plain(t::inbox::pill_applied().to_owned(), Tone::Good),
            OutcomeState::Failed => plain(
                match outcome.reason.as_deref().filter(|why| !why.is_empty()) {
                    Some(why) => t::inbox::pill_failed_why(why),
                    None => t::inbox::pill_failed().to_owned(),
                },
                Tone::Bad,
            ),
            OutcomeState::Withdrawn => plain(t::inbox::pill_withdrawn().to_owned(), Tone::Bad),
        };
    }
    if sent_back(item).is_some() {
        return plain(t::inbox::pill_sent_back().to_owned(), Tone::Plain);
    }
    if item.state != State::Open {
        let tone = if item.state == State::Expired {
            Tone::Bad
        } else {
            Tone::Plain
        };
        return plain(result.unwrap_or_default().to_owned(), tone);
    }
    let tone = if item.is_blocking() {
        Tone::Warn
    } else {
        Tone::Plain
    };
    let Some(ends) = item.expires_ms else {
        return plain(t::inbox::pill_no_end().to_owned(), tone);
    };
    let left = (ends - now_ms).max(0);
    let span = (ends - item.created_ms).max(1);
    Pill {
        text: t::inbox::pill_ends_in(&duration(left as f64 / 1000.0)),
        tone,
        drain: Some((left as f32 / span as f32).clamp(0.0, 1.0)),
    }
}

fn place_chips(item: &Item) -> (Option<String>, Option<String>) {
    let Some(place) = &item.place else {
        return (None, None);
    };
    let session = match (&place.session_name, &place.branch) {
        (Some(name), Some(branch)) => Some(t::inbox::chip_session(name, branch)),
        (Some(name), None) => Some(name.clone()),
        (None, Some(branch)) => Some(branch.clone()),
        (None, None) => None,
    };
    (place.machine_name.clone(), session)
}

pub(crate) fn words_of(item: &Item) -> (String, String) {
    let trusted = notchling_engine::consent::worker_switch(item).and_then(|asked| {
        let (title, unlocks) = super::consent_card::switch_words(&asked.switch)?;
        let machine = item
            .place
            .as_ref()
            .and_then(|place| place.machine_name.clone())
            .unwrap_or_else(|| asked.node.chars().take(MACHINE_CHARS).collect());
        Some((
            t::inbox::worker_switch_title(title, &machine),
            unlocks.to_owned(),
        ))
    });
    trusted.unwrap_or_else(|| (item.title.clone(), item.detail.clone()))
}

pub(super) fn row_of(item: &Item, now_ms: i64, section: Section) -> Row {
    let result = result_of(item);
    let (title, said) = words_of(item);
    let detail = match (&result, sent_back(item)) {
        (_, Some(note)) => t::inbox::sent_back_note(note),
        (Some(word), _) if said.is_empty() => word.clone(),
        (Some(word), _) => [said.as_str(), word.as_str()].join(SEP),
        (None, _) => said,
    };
    let (machine, session) = place_chips(item);
    let waited = (item.state == State::Open
        && item.is_blocking()
        && !super::triage::is_applying(item))
    .then(|| t::inbox::stopped_for(&age(now_ms, item.waiting_since.unwrap_or(item.created_ms))));
    Row {
        section,
        kind: item.kind,
        unseen: item.state == State::Open && !item.seen && !super::triage::is_applying(item),
        blocking: item.is_blocking() && !super::triage::is_applying(item),
        machine,
        session,
        waited,
        pill: pill_of(item, result.as_deref(), now_ms),
        change: change_route(item),
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
        enabled: item.state == State::Open && !super::triage::is_applying(item),
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
            cleared: HashSet::new(),
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
                .map(|item| row_of(item, self.now_ms, section_of(item, self.now_ms)))
                .collect()
        };
        (rows(needs), rows(done))
    }

    pub fn unread(&self) -> usize {
        self.items
            .iter()
            .filter(|item| {
                matches!(
                    section_of(item, self.now_ms),
                    Section::Blocking | Section::Waiting
                )
            })
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

pub fn consent_deny_with_args(id: &str, note: &str) -> Value {
    json!({"consentId": id, "decision": "deny", "note": note})
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
