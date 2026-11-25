use notchling_engine::cluster::{qualify, NodeId};
use serde_json::{json, Map, Value};

pub const ID_MAX: usize = 128;
pub const TEXT_MAX: usize = 4_096;
pub const INPUT_MAX: usize = 64 * 1024;
pub const QUESTION_MAX: usize = 128 * 1024;
const ITEM_FIELDS: [&str; 5] = ["header", "question", "options", "details", "multiSelect"];

#[derive(Default)]
pub struct Found {
    pub mirror: Option<String>,
    pub row: Option<String>,
}

pub type Finder<'a> = &'a dyn Fn(&NodeId, &str, &str) -> Found;

fn bare(value: Option<&Value>, allow_empty: bool) -> Option<String> {
    let text = value?.as_str()?;
    let fine = text.len() <= ID_MAX
        && !text.contains('/')
        && !text.chars().any(char::is_control)
        && (allow_empty || !text.is_empty());
    fine.then(|| text.to_string())
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(TEXT_MAX)
        .collect()
}

fn within(value: &Value, max: usize) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= max)
}

fn clean_question(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let items = value.get("items")?.as_array()?;
    let mut kept = Vec::new();
    for item in items {
        let fields = item.as_object()?;
        let mut clean = Map::new();
        for key in ITEM_FIELDS {
            if let Some(v) = fields.get(key) {
                clean.insert(key.to_string(), v.clone());
            }
        }
        kept.push(Value::Object(clean));
    }
    let out = json!({"items": kept});
    within(&out, QUESTION_MAX).then_some(out)
}

pub fn clean_pending(node: &NodeId, raw: &Value, find: Finder) -> Option<Value> {
    let fields = raw.as_object()?;
    let id = bare(fields.get("id"), false)?;
    let session = bare(fields.get("sessionId"), true)?;
    let kind = match fields.get("kind").and_then(Value::as_str)? {
        kind @ ("permission" | "question") => kind,
        _ => return None,
    };
    let question = clean_question(fields.get("question").unwrap_or(&Value::Null))?;
    let mut trimmed = fields.get("trimmed").and_then(Value::as_bool) == Some(true);
    let input = match fields.get("input") {
        Some(v) if v.is_object() && within(v, INPUT_MAX) => v.clone(),
        other => {
            trimmed |= other.is_some_and(|v| !v.is_null());
            json!({})
        }
    };
    let owner_path = text(fields.get("transcriptPath"));
    let found = find(node, &owner_path, &session);
    let mut out = json!({
        "id": qualify(node, &id),
        "sessionId": if session.is_empty() { String::new() } else { qualify(node, &session) },
        "kind": kind,
        "toolName": text(fields.get("toolName")),
        "summary": text(fields.get("summary")),
        "input": input,
        "canAlwaysAllow": false,
        "question": question,
        "cwd": text(fields.get("cwd")),
        "transcriptPath": found.mirror,
        "createdMs": fields.get("createdMs").and_then(Value::as_i64).unwrap_or(0),
        "node": node,
    });
    if trimmed {
        out["trimmed"] = json!(true);
    }
    if let Some(row) = found.row {
        out["rowId"] = json!(row);
    }
    crate::prompts::push_row(&out)
}

pub fn clean_resolved(node: &NodeId, raw: &Value) -> Option<(String, Value)> {
    let fields = raw.as_object()?;
    let id = bare(fields.get("id"), false)?;
    let session = bare(fields.get("sessionId"), true)?;
    let qualified = qualify(node, &id);
    let short = |key: &str| -> String { text(fields.get(key)).chars().take(32).collect() };
    let data = json!({
        "id": qualified,
        "sessionId": if session.is_empty() { String::new() } else { qualify(node, &session) },
        "outcome": short("outcome"),
        "by": short("by"),
        "node": node,
    });
    Some((qualified, data))
}
