use notchling_engine::consent::{Consent, State};
use serde_json::{json, Map, Value};

pub(crate) fn view(row: &Consent) -> Value {
    let mut scope = Map::new();
    scope.insert("once".into(), json!(true));
    if let Some(session) = &row.session {
        scope.insert("session".into(), json!(session));
    }
    scope.insert("until".into(), json!(row.expires_ms));
    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("kind".into(), json!(row.kind));
    out.insert("target".into(), json!(row.target));
    out.insert("change".into(), row.change.clone());
    out.insert("facts".into(), row.facts.clone());
    if let Some(parts) = &row.parts {
        out.insert("parts".into(), parts.clone());
    }
    if let Some(allowed) = &row.parts_allowed {
        out.insert("partsAllowed".into(), allowed.clone());
    }
    out.insert("state".into(), json!(row.state.as_str()));
    out.insert("answerOn".into(), json!(row.answer_on.as_str()));
    out.insert("appliedBy".into(), json!(row.applied_by.as_str()));
    out.insert("createdMs".into(), json!(row.created_ms));
    out.insert("expiresMs".into(), json!(row.expires_ms));
    if let Some(at) = row.answered_ms {
        out.insert("answeredMs".into(), json!(at));
    }
    if let Some(by) = &row.by {
        out.insert("by".into(), json!(by));
    }
    if let Some(at) = row.used_ms {
        out.insert("usedMs".into(), json!(at));
    }
    if let Some(session) = &row.session {
        out.insert("session".into(), json!(session));
    }
    out.insert("tool".into(), json!(row.tool));
    out.insert("verified".into(), json!(row.verified));
    if let Some(organization) = &row.organization {
        out.insert("organization".into(), json!(organization));
    }
    if let Some(folder) = &row.folder {
        out.insert("folder".into(), json!(folder));
    }
    if let Some(root) = &row.root {
        out.insert("root".into(), json!(root));
    }
    out.insert("scope".into(), Value::Object(scope));
    match &row.result {
        Some(result) if row.state == State::Failed && result.get("error").is_some() => {
            out.insert("error".into(), result["error"].clone());
        }
        Some(result) => {
            out.insert("result".into(), result.clone());
        }
        None => {}
    }
    if let Some(reason) = &row.reason {
        out.insert("reason".into(), json!(reason));
    }
    if let Some(note) = &row.note {
        out.insert("note".into(), json!(note));
    }
    Value::Object(out)
}

pub(crate) fn ask_message(row: &Consent) -> &'static str {
    match row.state {
        State::Waiting | State::Opened => "Waiting for the user to allow it in Notchling.",
        State::Applying => "Allowed; applying it.",
        State::Applied => "The user allowed it, and it's done.",
        State::Denied if row.note.is_some() => "The user denied it and asked for a change.",
        State::Denied => "The user denied it.",
        State::Expired => "The user didn't answer in time.",
        State::Cancelled => "The request was withdrawn.",
        State::Withdrawn => "The request was withdrawn before it was applied.",
        State::Failed => "It couldn't be applied.",
    }
}
