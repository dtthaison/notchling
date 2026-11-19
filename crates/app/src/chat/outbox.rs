use std::collections::{BTreeMap, HashMap, VecDeque};

use notchling_engine::outbox::{Kind, State, Tool, Via, Why};
use notchling_engine::sessions::Agent;
use notchling_engine::terminal::input_box;
use notchling_engine::transcript::model::Conversation;
use notchling_local_api::{topics, Event};
use serde_json::Value;

use super::outgoing::{dated_since, Outgoing};
use crate::acting::Request;

const SENT_KEPT: usize = 50;
const GONE_KEPT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: String,
    pub session: String,
    pub tool: Tool,
    pub kind: Kind,
    pub text: String,
    pub state: State,
    pub why: Option<Why>,
    pub boxed: Option<String>,
    pub error: Option<String>,
    pub position: usize,
    pub queued_ms: i64,
    pub updated_ms: i64,
    pub attempt_ms: Option<i64>,
    pub via: Via,
}

fn word<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl OutboxRow {
    pub fn from_wire(value: &Value) -> Option<OutboxRow> {
        Some(OutboxRow {
            id: word(value, "id")?.to_owned(),
            session: word(value, "session")?.to_owned(),
            tool: Tool::parse(word(value, "tool")?)?,
            kind: Kind::parse(word(value, "kind")?)?,
            text: word(value, "text")?.to_owned(),
            state: State::parse(word(value, "state")?)?,
            why: word(value, "why").and_then(Why::parse),
            boxed: word(value, "box").map(str::to_owned),
            error: word(value, "error").map(str::to_owned),
            position: value
                .get("position")
                .and_then(Value::as_u64)
                .map_or(1, |n| n as usize),
            queued_ms: value.get("queuedMs").and_then(Value::as_i64).unwrap_or(0),
            updated_ms: value.get("updatedMs").and_then(Value::as_i64).unwrap_or(0),
            attempt_ms: value.get("attemptMs").and_then(Value::as_i64),
            via: Via::parse(word(value, "via").unwrap_or("chat")).unwrap_or(Via::Chat),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    QueuedRetry,
    WaitingDialog,
    WaitsForTurn,
    Sending,
    QueuedInSession,
    Sent,
    MayBeIn,
    BoxHasText(String),
    BoxHoldsThis,
    Stopped,
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Edit,
    Remove,
    SendAgain,
    Drop,
    ClearBoxAndSend,
    ResumeAndSend,
}

impl Button {
    pub fn request(self, id: &str, session: &str) -> Option<Request> {
        match self {
            Button::Edit => None,
            Button::Remove | Button::Drop => Some(Request::outbox_remove(id, session)),
            Button::SendAgain => Some(Request::outbox_resend(id, session, false, false)),
            Button::ClearBoxAndSend => Some(Request::outbox_resend(id, session, true, false)),
            Button::ResumeAndSend => Some(Request::outbox_resend(id, session, false, true)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bubble {
    pub id: String,
    pub text: String,
    pub status: Status,
    pub buttons: Vec<Button>,
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn bubble(row: &OutboxRow) -> Bubble {
    let (status, buttons) = match row.state {
        State::Queued => (Status::Queued, vec![Button::Edit, Button::Remove]),
        State::Waiting => {
            let status = match row.why {
                Some(Why::Retry) => Status::QueuedRetry,
                Some(Why::Dialog) => Status::WaitingDialog,
                Some(Why::Turn) => Status::WaitsForTurn,
                _ => Status::Queued,
            };
            (status, vec![Button::Edit, Button::Remove])
        }
        State::Sending => (Status::Sending, Vec::new()),
        State::InQueue => (Status::QueuedInSession, Vec::new()),
        State::Unsure => (Status::MayBeIn, vec![Button::SendAgain, Button::Drop]),
        State::Box => {
            let boxed = row.boxed.as_deref().unwrap_or_default();
            let held = squeeze(boxed);
            let same = held == squeeze(&row.text) || input_box::holds_message(boxed, &row.text);
            let status = if !held.is_empty() && same {
                Status::BoxHoldsThis
            } else {
                Status::BoxHasText(held)
            };
            (
                status,
                vec![Button::ClearBoxAndSend, Button::Edit, Button::Remove],
            )
        }
        State::Stopped => (
            Status::Stopped,
            vec![Button::ResumeAndSend, Button::Edit, Button::Remove],
        ),
        State::Gone => (Status::NotFound, vec![Button::Remove]),
    };
    Bubble {
        id: row.id.clone(),
        text: row.text.clone(),
        status,
        buttons,
    }
}

pub fn head_needs_you(rows: &[OutboxRow]) -> bool {
    rows.first().is_some_and(|head| {
        matches!(
            head.state,
            State::Unsure | State::Box | State::Stopped | State::Gone
        )
    })
}

fn shown_by_transcript(row: &OutboxRow, conversation: &Conversation) -> bool {
    let since = match row.state {
        State::Sending => row.updated_ms,
        State::Unsure => row.attempt_ms.unwrap_or(row.queued_ms),
        _ => return false,
    };
    dated_since(&row.text, since as f64, conversation)
}

pub fn hidden_queued(queued: &[String], rows: &[OutboxRow]) -> Option<usize> {
    let held = rows.iter().find(|r| r.state == State::InQueue)?;
    let wanted = squeeze(&held.text);
    queued
        .iter()
        .position(|entry| squeeze(entry) == wanted || input_box::shown_as_chips(&held.text, entry))
}

pub fn unbubbled(outgoing: Vec<String>, bubbles: &[Bubble]) -> Vec<String> {
    let mut covered: Vec<String> = bubbles.iter().map(|b| squeeze(&b.text)).collect();
    outgoing
        .into_iter()
        .filter(
            |text| match covered.iter().position(|c| *c == squeeze(text)) {
                Some(at) => {
                    covered.remove(at);
                    false
                }
                None => true,
            },
        )
        .collect()
}

pub fn deliver(list: &mut [Outgoing], text: &str, now_ms: i64, row: Option<&str>) {
    if let Some(out) = list
        .iter_mut()
        .find(|o| o.text == text && o.delivered_at.is_none())
    {
        out.delivered(now_ms);
        out.boxed = row.map(str::to_owned);
    }
}

pub fn drop_boxed(list: &mut Vec<Outgoing>, shows: impl Fn(&str) -> bool) {
    list.retain(|o| !o.boxed.as_deref().is_some_and(&shows));
}

pub fn drop_covered(lists: &mut HashMap<String, Vec<Outgoing>>, covers: impl Fn(&str) -> bool) {
    for list in lists.values_mut() {
        drop_boxed(list, &covers);
    }
}

pub fn forget(list: &mut Vec<Outgoing>, row: &str) {
    list.retain(|o| o.boxed.as_deref() != Some(row));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SentRow {
    row: OutboxRow,
    at_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutboxFeed {
    rows: BTreeMap<String, Vec<OutboxRow>>,
    sent: BTreeMap<String, Vec<SentRow>>,
    gone: VecDeque<String>,
}

fn queue_order(row: &OutboxRow) -> (i64, usize) {
    (row.queued_ms, row.position)
}

fn place(list: &mut Vec<OutboxRow>, row: OutboxRow) {
    let key = queue_order(&row);
    let at = list
        .iter()
        .position(|known| queue_order(known) > key)
        .unwrap_or(list.len());
    list.insert(at, row);
}

impl OutboxFeed {
    pub fn seed(&mut self, list: &[Value]) {
        self.rows.clear();
        for row in list.iter().filter_map(OutboxRow::from_wire) {
            place(self.rows.entry(row.session.clone()).or_default(), row);
        }
        self.prune_sent();
    }

    pub fn clear_rows(&mut self) {
        self.rows.clear();
    }

    pub fn reseed_rows(&mut self, fresh: OutboxFeed) {
        self.rows = fresh.rows;
        for (session, list) in fresh.sent {
            self.sent.entry(session).or_default().extend(list);
        }
        self.prune_sent();
    }

    pub fn shows(&self, id: &str) -> bool {
        self.rows.values().flatten().any(|row| row.id == id)
            || self.sent.values().flatten().any(|kept| kept.row.id == id)
    }

    pub fn covers(&self, id: &str) -> bool {
        self.shows(id) || self.gone.iter().any(|gone| gone == id)
    }

    fn remember_gone(&mut self, id: &str) -> bool {
        if self.gone.iter().any(|gone| gone == id) {
            return false;
        }
        self.gone.push_back(id.to_owned());
        if self.gone.len() > GONE_KEPT {
            self.gone.pop_front();
        }
        true
    }

    fn prune_sent(&mut self) -> bool {
        let rows = &self.rows;
        let before: usize = self.sent.values().map(Vec::len).sum();
        for list in self.sent.values_mut() {
            list.retain(|kept| !rows.values().flatten().any(|row| row.id == kept.row.id));
        }
        self.sent.retain(|_, list| !list.is_empty());
        before != self.sent.values().map(Vec::len).sum::<usize>()
    }

    pub fn rows(&self, session: &str) -> &[OutboxRow] {
        self.rows.get(session).map_or(&[], Vec::as_slice)
    }

    pub fn rows_for(&self, agent: &Agent) -> &[OutboxRow] {
        self.rows(&agent.outbox_key())
    }

    pub fn needs_you(&self, agent: &Agent) -> bool {
        head_needs_you(self.rows_for(agent))
    }

    pub fn bubbles_for(&self, agent: &Agent) -> Vec<Bubble> {
        self.bubbles(&agent.outbox_key())
    }

    pub fn settle_for(&mut self, agent: &Agent, conversation: &Conversation, now_ms: i64) -> bool {
        self.settle(&agent.outbox_key(), conversation, now_ms)
    }

    pub fn apply(&mut self, event: &Event, now_ms: i64) -> bool {
        match event.event.as_str() {
            topics::OUTBOX_CHANGED => event
                .data
                .get("row")
                .and_then(OutboxRow::from_wire)
                .is_some_and(|row| self.upsert(row)),
            topics::OUTBOX_REMOVED => {
                let Some(id) = word(&event.data, "id") else {
                    return false;
                };
                if word(&event.data, "why") == Some("sent") {
                    return self.sent(id, now_ms);
                }
                let taken = self.take(id).is_some();
                self.remember_gone(id) || taken
            }
            _ => false,
        }
    }

    fn upsert(&mut self, row: OutboxRow) -> bool {
        let list = self.rows.entry(row.session.clone()).or_default();
        let changed = match list.iter_mut().find(|r| r.id == row.id) {
            Some(known) if *known == row => false,
            Some(known) => {
                *known = row;
                true
            }
            None => {
                place(list, row);
                true
            }
        };
        self.prune_sent() || changed
    }

    fn take(&mut self, id: &str) -> Option<OutboxRow> {
        let mut found = None;
        for list in self.rows.values_mut() {
            if let Some(at) = list.iter().position(|r| r.id == id) {
                found = Some(list.remove(at));
                break;
            }
        }
        self.rows.retain(|_, list| !list.is_empty());
        found
    }

    pub fn sent(&mut self, id: &str, now_ms: i64) -> bool {
        let Some(row) = self.take(id) else {
            return false;
        };
        let list = self.sent.entry(row.session.clone()).or_default();
        list.push(SentRow { row, at_ms: now_ms });
        if list.len() > SENT_KEPT {
            list.remove(0);
        }
        true
    }

    pub fn settle(&mut self, session: &str, conversation: &Conversation, now_ms: i64) -> bool {
        let Some(list) = self.sent.get_mut(session) else {
            return false;
        };
        let before = list.len();
        list.retain(|kept| {
            let waiting = Outgoing {
                text: kept.row.text.clone(),
                seen: usize::MAX,
                delivered_at: Some(kept.at_ms),
                sent_at: kept.row.queued_ms,
                boxed: None,
            };
            waiting.keep(conversation, now_ms)
        });
        let changed = list.len() != before;
        if list.is_empty() {
            self.sent.remove(session);
        }
        changed
    }

    pub fn bubbles(&self, session: &str) -> Vec<Bubble> {
        self.bubbles_in(session, &Conversation::default())
    }

    pub fn bubbles_for_in(&self, agent: &Agent, conversation: &Conversation) -> Vec<Bubble> {
        self.bubbles_in(&agent.outbox_key(), conversation)
    }

    pub fn bubbles_in(&self, session: &str, conversation: &Conversation) -> Vec<Bubble> {
        let sent = self
            .sent
            .get(session)
            .into_iter()
            .flatten()
            .map(|kept| Bubble {
                id: kept.row.id.clone(),
                text: kept.row.text.clone(),
                status: Status::Sent,
                buttons: Vec::new(),
            });
        let waiting = self
            .rows(session)
            .iter()
            .filter(|row| !shown_by_transcript(row, conversation))
            .map(bubble);
        sent.chain(waiting).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.sent.is_empty()
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
