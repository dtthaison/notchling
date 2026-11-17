use std::collections::HashMap;

use notchling_engine::sessions::Agent;
use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem};
use notchling_locale as t;
use serde_json::Value;

use super::model::NotchModel;
use crate::acting::{Answer, AskAnswer, Request};
use crate::chat::asks::{lost_line, AskRow};

pub const KEY_PREFIX: &str = "ask:";

#[derive(Clone, Debug, PartialEq)]
pub struct AskCard {
    pub id: String,
    pub question: PendingQuestion,
    pub position: usize,
    pub total: usize,
}

impl AskCard {
    pub fn step(&self) -> Option<String> {
        (self.total > 1).then(|| t::notch::question_step(self.position, self.total))
    }
}

fn pending(row: &AskRow) -> PendingQuestion {
    PendingQuestion {
        tool_use_id: format!("{KEY_PREFIX}{}", row.id),
        items: vec![QuestionItem {
            header: row.label.clone().unwrap_or_default(),
            question: row.question.clone(),
            options: row.choices.clone(),
            details: vec![String::new(); row.choices.len()],
            multi_select: false,
        }],
    }
}

pub fn cards(agents: &[Agent], rows: &[AskRow]) -> HashMap<String, AskCard> {
    let mut out = HashMap::new();
    for agent in agents {
        let key = agent.outbox_key();
        let mut mine: Vec<&AskRow> = rows
            .iter()
            .filter(|r| r.is_open() && r.session == key)
            .collect();
        mine.sort_by(|a, b| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
        let Some(first) = mine.first() else {
            continue;
        };
        out.insert(
            agent.id.clone(),
            AskCard {
                id: first.id.clone(),
                question: pending(first),
                position: 1,
                total: mine.len(),
            },
        );
    }
    out
}

pub fn is_ask(tool_use_id: &str) -> bool {
    tool_use_id.starts_with(KEY_PREFIX)
}

pub fn answer_key(model: &NotchModel, session: &str, q: &PendingQuestion) -> Option<String> {
    if is_ask(&q.tool_use_id) {
        return Some(q.tool_use_id.clone());
    }
    model.answer_key(session)
}

pub fn lost_reply(key: Option<&str>, reply: &Value) -> Option<String> {
    let asked = key.is_some_and(|k| k.starts_with(KEY_PREFIX)) || reply.get("answered").is_some();
    if asked {
        lost_line(reply)
    } else {
        None
    }
}

pub fn request_for(session: &str, q: &PendingQuestion, answers: &[Answer]) -> Request {
    if let Some(id) = q.tool_use_id.strip_prefix(KEY_PREFIX) {
        match answers.first() {
            Some(Answer::Options(picked)) => {
                if let Some(n) = picked.first().and_then(|n| u8::try_from(*n).ok()) {
                    return Request::ask_answer(id, AskAnswer::Choice(n));
                }
            }
            Some(Answer::Text(text)) => return Request::ask_answer(id, AskAnswer::Text(text)),
            None => {}
        }
    }
    Request::answer_for(session, q, answers)
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod tests;
