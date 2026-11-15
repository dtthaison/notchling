use super::model::Session;
use super::wire::BridgeFailure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op")]
pub enum HeldAct {
    #[serde(rename = "session.message")]
    Message {
        #[serde(rename = "sessionId")]
        session_id: String,
        text: String,
    },
    #[serde(rename = "session.sendNow")]
    SendNow {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    #[serde(rename = "prompt.answer")]
    Answer {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        allow: Option<bool>,
        answers: Option<Vec<String>>,
    },
    #[serde(rename = "session.new")]
    NewSession {
        cwd: String,
        model: Option<String>,
        #[serde(rename = "modelId")]
        model_id: Option<String>,
    },
}

const SUMMARY_CHARS: usize = 80;

fn shorten(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= SUMMARY_CHARS {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(SUMMARY_CHARS).collect();
    format!("{cut}...")
}

impl HeldAct {
    pub fn op(&self) -> &'static str {
        match self {
            HeldAct::Message { .. } => "session.message",
            HeldAct::SendNow { .. } => "session.sendNow",
            HeldAct::Answer { .. } => "prompt.answer",
            HeldAct::NewSession { .. } => "session.new",
        }
    }

    pub fn args(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(map) = value.as_object_mut() {
            map.remove("op");
        }
        value
    }

    pub fn from_parts(op: &str, args: &Value) -> Option<HeldAct> {
        let mut value = args.clone();
        value.as_object_mut()?.insert("op".into(), op.into());
        serde_json::from_value(value).ok()
    }

    pub fn session_id(&self) -> Option<&str> {
        match self {
            HeldAct::Message { session_id, .. }
            | HeldAct::SendNow { session_id }
            | HeldAct::Answer { session_id, .. } => Some(session_id),
            HeldAct::NewSession { .. } => None,
        }
    }

    pub fn summary(&self) -> String {
        match self {
            HeldAct::Message { text, .. } => shorten(text),
            HeldAct::SendNow { .. } => "Interrupt".to_string(),
            HeldAct::Answer { allow, answers, .. } => match (allow, answers) {
                (Some(true), _) => "Allow".to_string(),
                (Some(false), _) => "Deny".to_string(),
                (None, Some(list)) => shorten(&list.join(", ")),
                (None, None) => "Answer".to_string(),
            },
            HeldAct::NewSession { cwd, .. } => shorten(&format!("New session in {cwd}")),
        }
    }
}

pub struct Call<'a> {
    pub device: &'a str,
    pub request: &'a str,
    pub act: &'a HeldAct,
    pub session: Option<&'a Session>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeldRef {
    pub id: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Routed {
    Local,
    Relayed,
    Held(HeldRef),
    Done(Value),
    Fail(BridgeFailure),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeldView {
    pub id: String,
    pub op: String,
    pub session_id: Option<String>,
    pub summary: String,
    pub at_ms: i64,
    pub waiting: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeldNoticeView {
    pub id: String,
    pub reason: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeldList {
    pub entries: Vec<HeldView>,
    pub notices: Vec<HeldNoticeView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    Dropped,
    Delivered,
    Unknown,
}

pub trait Relay: Send + Sync {
    fn cluster(&self) -> Option<Value>;

    fn route(&self, call: &Call<'_>) -> Routed;

    fn list(&self) -> HeldList;

    fn cancel(&self, id: &str) -> Cancelled;

    fn confirm(&self, id: &str) -> bool;

    fn stale_ms(&self) -> Option<i64>;

    fn set_transcript(&self, _session: &Session) -> Option<PathBuf> {
        None
    }

    fn tick(&self) {}
}

pub fn entry_json(entry: &HeldView) -> Value {
    let mut value = json!({
        "id": entry.id,
        "op": entry.op,
        "summary": entry.summary,
        "at": entry.at_ms,
        "waiting": entry.waiting,
    });
    if let Some(session) = &entry.session_id {
        value["sessionId"] = Value::from(session.as_str());
    }
    value
}

pub fn list_json(list: &HeldList) -> Value {
    let mut value = json!({
        "entries": list.entries.iter().map(entry_json).collect::<Vec<_>>(),
    });
    if !list.notices.is_empty() {
        value["notices"] = Value::Array(
            list.notices
                .iter()
                .map(|n| json!({"id": n.id, "reason": n.reason, "at": n.at_ms}))
                .collect(),
        );
    }
    value
}

pub fn held_json(held: &HeldRef) -> Value {
    json!({"held": {"id": held.id, "at": held.at_ms}})
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
