use notchling_engine::outbox::{State, Why};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use serde_json::Value;

use super::markers::{ANSWER_PREFIX, APPROVE};
use super::outbox::OutboxRow;
use super::remote;
use crate::acting::Request;
use crate::chat::data::Tool;

pub const READ_TRIES: u8 = 3;
pub const READ_RETRY_MS: i64 = 3_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogOption {
    pub number: u64,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogCard {
    pub key: String,
    pub title: String,
    pub body: Vec<String>,
    pub options: Vec<DialogOption>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.trim().is_empty())
}

impl DialogCard {
    pub fn from_reply(key: &str, reply: &Value) -> Option<DialogCard> {
        let dialog = reply.get("dialog").filter(|d| d.is_object())?;
        let options: Vec<DialogOption> = dialog
            .get("options")?
            .as_array()?
            .iter()
            .filter_map(|o| {
                Some(DialogOption {
                    number: o.get("number")?.as_u64()?,
                    label: text(o, "label")?,
                    selected: o.get("selected").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect();
        if options.len() < 2 {
            return None;
        }
        Some(DialogCard {
            key: key.to_owned(),
            title: text(dialog, "title")?,
            body: dialog
                .get("body")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            options,
        })
    }

    pub fn pick(&self, session: &str, option: &DialogOption) -> Request {
        Request::dialog_pick(session, &self.title, option.number, &option.label)
    }

    pub fn dismiss(&self, session: &str) -> Request {
        Request::dialog_dismiss(session)
    }
}

pub fn asked_by_outbox(rows: &[OutboxRow]) -> bool {
    rows.first().is_some_and(|head| {
        head.state == State::Waiting && matches!(head.why, Some(Why::Dialog | Why::Answer))
    })
}

pub fn read_key(agent: &Agent, tool: Tool, outbox_asks: bool) -> Option<String> {
    if tool != Tool::Claude
        || agent.command.is_some()
        || remote::is_remote(agent)
        || !matches!(agent.target, AgentTarget::Job(_))
    {
        return None;
    }
    let waiting = agent.group == AgentGroup::NeedsInput
        && !agent
            .detail
            .as_deref()
            .is_some_and(|d| d.starts_with(ANSWER_PREFIX) || d.starts_with(APPROVE));
    (waiting || outbox_asks).then(|| format!("dialog:{}|{}", agent.id, agent.since_ms))
}

#[derive(Debug, Default)]
pub struct Reading {
    pub card: Option<DialogCard>,
    pub asked: Option<String>,
    pub tries: u8,
    pub retry_ms: i64,
    pub picked: Option<u64>,
}

impl Reading {
    pub fn due(&self, key: &str, now_ms: i64) -> bool {
        match self.asked.as_deref() {
            Some(asked) if asked == key => {
                self.card.is_none() && self.tries < READ_TRIES && now_ms >= self.retry_ms
            }
            _ => true,
        }
    }

    pub fn begin(&mut self, key: &str) {
        if self.asked.as_deref() != Some(key) {
            self.card = None;
            self.tries = 0;
        }
        self.asked = Some(key.to_owned());
        self.tries += 1;
        self.retry_ms = i64::MAX;
    }

    pub fn finish(&mut self, key: &str, card: Option<DialogCard>, now_ms: i64) {
        if self.asked.as_deref() != Some(key) {
            return;
        }
        self.retry_ms = now_ms + READ_RETRY_MS;
        if card.is_some() {
            self.card = card;
        }
    }

    pub fn shown(&self, key: &str) -> Option<&DialogCard> {
        self.card.as_ref().filter(|card| card.key == key)
    }
}

pub fn preview() -> DialogCard {
    DialogCard {
        key: "dialog:preview".into(),
        title: "Switch model?".into(),
        body: vec![
            "Your next response will be slower and use more tokens".into(),
            "This conversation is cached for the current model. Switching to Opus 5.5 means the full history gets re-read.".into(),
        ],
        options: vec![
            DialogOption {
                number: 1,
                label: "Yes, switch to Opus 5.5".into(),
                selected: true,
            },
            DialogOption {
                number: 2,
                label: "No, go back".into(),
                selected: false,
            },
        ],
    }
}

#[cfg(test)]
#[path = "dialog_card_tests.rs"]
mod tests;
