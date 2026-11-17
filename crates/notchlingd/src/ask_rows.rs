use notchling_engine::ask::Row;
use notchling_engine::sessions::Agent;
use serde_json::{json, Map, Value};

fn put<T: Into<Value>>(out: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        out.insert(key.to_string(), value.into());
    }
}

pub(crate) fn wire(row: &Row, waiting: bool, with_answer: bool) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("session".into(), json!(row.session));
    put(&mut out, "sessionId", row.session_id.as_deref());
    out.insert("asker".into(), json!(row.asker.as_str()));
    put(&mut out, "agentId", row.agent_id.as_deref());
    put(&mut out, "label", row.label.as_deref());
    put(&mut out, "agentType", row.agent_type.as_deref());
    put(&mut out, "model", row.model.as_deref());
    put(&mut out, "folder", row.folder.as_deref());
    put(&mut out, "branch", row.branch.as_deref());
    out.insert("question".into(), json!(row.question));
    put(&mut out, "choices", row.choices.clone());
    out.insert("mainMayAnswer".into(), json!(row.main_may_answer));
    out.insert("mode".into(), json!(row.mode.as_str()));
    out.insert("state".into(), json!(row.state.as_str()));
    out.insert("waiting".into(), json!(waiting));
    if with_answer {
        put(&mut out, "answer", row.answer.as_deref());
    }
    put(&mut out, "choice", row.choice);
    put(&mut out, "by", row.by.map(|by| by.as_str()));
    out.insert("createdMs".into(), json!(row.created_ms));
    out.insert("updatedMs".into(), json!(row.updated_ms));
    put(&mut out, "closedMs", row.closed_ms);
    Value::Object(out)
}

fn agent_of<'a>(agents: &'a [Agent], session_id: &str) -> Option<&'a Agent> {
    agents
        .iter()
        .find(|agent| agent.session_id().as_deref() == Some(session_id) || agent.id == session_id)
}

pub(crate) fn key_for(agents: &[Agent], session_id: &str) -> String {
    agent_of(agents, session_id).map_or_else(|| session_id.to_string(), Agent::outbox_key)
}

pub(crate) fn name_for(agents: &[Agent], session_id: &str) -> String {
    agent_of(agents, session_id)
        .map(|agent| agent.name.clone())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "ask_rows_tests.rs"]
mod tests;
