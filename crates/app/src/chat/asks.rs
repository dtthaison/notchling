use std::collections::BTreeMap;

use notchling_engine::ask::{AskState, Asker, By};
use notchling_engine::sessions::Agent;
use notchling_local_api::{ops, topics, Client, ClientError};
use notchling_locale as t;
use serde_json::Value;

const CLOSED_KEPT_MS: i64 = 600_000;
const ANSWER_CHARS: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskRow {
    pub id: String,
    pub session: String,
    pub session_id: Option<String>,
    pub asker: Asker,
    pub agent_id: Option<String>,
    pub label: Option<String>,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub folder: Option<String>,
    pub branch: Option<String>,
    pub question: String,
    pub choices: Vec<String>,
    pub main_may_answer: bool,
    pub state: AskState,
    pub waiting: bool,
    pub answer: Option<String>,
    pub choice: Option<u8>,
    pub by: Option<By>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
}

fn word<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn text(value: &Value, key: &str) -> Option<String> {
    word(value, key)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn millis(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

impl AskRow {
    pub fn from_wire(value: &Value) -> Option<AskRow> {
        Some(AskRow {
            id: word(value, "id")?.to_owned(),
            session: word(value, "session")?.to_owned(),
            session_id: text(value, "sessionId"),
            asker: word(value, "asker")
                .and_then(Asker::parse)
                .unwrap_or(Asker::Unknown),
            agent_id: text(value, "agentId"),
            label: text(value, "label"),
            agent_type: text(value, "agentType"),
            model: text(value, "model"),
            folder: text(value, "folder"),
            branch: text(value, "branch"),
            question: word(value, "question")?.to_owned(),
            choices: value
                .get("choices")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            main_may_answer: value
                .get("mainMayAnswer")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            state: AskState::parse(word(value, "state")?)?,
            waiting: value
                .get("waiting")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            answer: word(value, "answer").map(str::to_owned),
            choice: value
                .get("choice")
                .and_then(Value::as_u64)
                .and_then(|n| u8::try_from(n).ok()),
            by: word(value, "by").and_then(By::parse),
            created_ms: millis(value, "createdMs")?,
            updated_ms: millis(value, "updatedMs").unwrap_or(0),
            closed_ms: millis(value, "closedMs"),
        })
    }

    pub fn is_open(&self) -> bool {
        self.state == AskState::Open
    }

    fn closed_at(&self) -> i64 {
        self.closed_ms.unwrap_or(self.updated_ms)
    }

    fn answer_text(&self) -> Option<String> {
        let typed = self.answer.as_deref().filter(|a| !a.is_empty());
        let picked = || {
            let at = usize::from(self.choice?).checked_sub(1)?;
            self.choices.get(at).map(String::as_str)
        };
        typed.or_else(picked).map(squeeze)
    }
}

fn squeeze(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(ANSWER_CHARS) {
        Some((at, _)) => format!("{}…", flat[..at].trim_end()),
        None => flat,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskFeed {
    rows: BTreeMap<String, AskRow>,
}

impl AskFeed {
    pub fn apply(&mut self, event: &str, data: &Value) -> bool {
        match event {
            topics::ASK_CHANGED => {
                let Some(row) = data.get("row").and_then(AskRow::from_wire) else {
                    return false;
                };
                if self.rows.get(&row.id) == Some(&row) {
                    return false;
                }
                self.rows.insert(row.id.clone(), row);
                true
            }
            topics::ASK_REMOVED => {
                word(data, "id").is_some_and(|id| self.rows.remove(id).is_some())
            }
            _ => false,
        }
    }

    pub fn reseed(&mut self, rows: Vec<AskRow>) {
        self.rows = rows.into_iter().map(|row| (row.id.clone(), row)).collect();
    }

    pub fn rows_for(&self, agent: &Agent) -> Vec<&AskRow> {
        let key = agent.outbox_key();
        let mut rows: Vec<&AskRow> = self.rows.values().filter(|r| r.session == key).collect();
        rows.sort_by(|a, b| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
        rows
    }

    pub fn open_rows(&self) -> Vec<AskRow> {
        self.rows
            .values()
            .filter(|r| r.is_open())
            .cloned()
            .collect()
    }

    pub fn needs_you(&self, agent: &Agent) -> bool {
        let key = agent.outbox_key();
        self.rows
            .values()
            .any(|row| row.session == key && row.is_open())
    }
}

pub fn seed(client: &mut Client) -> Result<AskFeed, ClientError> {
    let mut feed = AskFeed::default();
    match client.subscribe(&[topics::ASKS]) {
        Ok(()) => {}
        Err(ClientError::Api(_)) => return Ok(feed),
        Err(e) => return Err(e),
    }
    match client.call(ops::ASK_LIST, serde_json::json!({})) {
        Ok(Value::Array(list)) => feed.reseed(list.iter().filter_map(AskRow::from_wire).collect()),
        Ok(_) | Err(ClientError::Api(_)) => {}
        Err(e) => return Err(e),
    }
    Ok(feed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub header: String,
    pub question: String,
    pub choices: Vec<String>,
    pub open: bool,
    pub closed_line: Option<String>,
}

pub fn waited(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match minutes {
        0 => t::chat::ask_waited_now().to_owned(),
        1..=59 => t::chat::ask_waited_minutes(minutes),
        _ => t::chat::ask_waited_hours(minutes / 60),
    }
}

fn who(by: By) -> String {
    match by {
        By::Mac => t::chat::ask_by_mac(),
        By::Phone => t::chat::ask_by_phone(),
        By::Main => t::chat::ask_by_main(),
    }
    .to_owned()
}

fn title(row: &AskRow) -> String {
    let named = row.label.clone();
    match row.asker {
        Asker::Main => t::chat::ask_main().to_owned(),
        Asker::Unknown => t::chat::ask_unknown().to_owned(),
        Asker::Subagent => named.unwrap_or_else(|| t::chat::ask_subagent().to_owned()),
        Asker::Codex => named.unwrap_or_else(|| t::chat::ask_codex().to_owned()),
        Asker::Antigravity => named.unwrap_or_else(|| t::chat::ask_antigravity().to_owned()),
    }
}

fn header(row: &AskRow, now_ms: i64) -> String {
    let waited = waited(now_ms - row.created_ms);
    [
        row.agent_type.clone(),
        row.model.clone(),
        row.branch.clone(),
        Some(waited),
    ]
    .into_iter()
    .flatten()
    .fold(title(row), |head, tail| t::chat::ask_join(&head, &tail))
}

fn closed_words(state: AskState, by: Option<By>, answer: Option<&str>) -> Option<String> {
    Some(match state {
        AskState::Open => return None,
        AskState::Answered | AskState::Delivered => match (by, answer) {
            (Some(by), Some(answer)) if by != By::Main => {
                t::chat::ask_answered_with(&who(by), answer)
            }
            (Some(by), _) => t::chat::ask_answered_by(&who(by)),
            (None, _) => t::chat::ask_answered().to_owned(),
        },
        AskState::Dismissed => t::chat::ask_dismissed().to_owned(),
        AskState::Expired => t::chat::ask_expired().to_owned(),
        AskState::Gone => t::chat::ask_gone().to_owned(),
    })
}

pub fn card(row: &AskRow, now_ms: i64) -> Option<Card> {
    let closed_line = closed_words(row.state, row.by, row.answer_text().as_deref());
    if closed_line.is_some() && now_ms - row.closed_at() >= CLOSED_KEPT_MS {
        return None;
    }
    let open = closed_line.is_none();
    Some(Card {
        header: header(row, now_ms),
        question: row.question.clone(),
        choices: if open {
            row.choices.clone()
        } else {
            Vec::new()
        },
        open,
        closed_line,
    })
}

pub fn lost_line(reply: &Value) -> Option<String> {
    if reply.get("answered").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let state = word(reply, "state").and_then(AskState::parse);
    let by = word(reply, "by").and_then(By::parse);
    Some(match (state, by) {
        (Some(AskState::Dismissed), _) => t::chat::ask_dismissed().to_owned(),
        (Some(AskState::Expired), _) => t::chat::ask_expired().to_owned(),
        (Some(AskState::Gone), _) => t::chat::ask_gone().to_owned(),
        (_, Some(by)) => t::chat::ask_already_by(&who(by)),
        _ => t::chat::ask_already().to_owned(),
    })
}

#[cfg(test)]
#[path = "asks_tests.rs"]
mod tests;
