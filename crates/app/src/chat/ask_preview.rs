use serde_json::{json, Value};

use super::asks::AskRow;

pub const STATES: [&str; 6] = [
    "open",
    "open-choices",
    "unknown",
    "answered",
    "dismissed",
    "gone",
];

fn row(state: &str, now_ms: i64) -> Option<AskRow> {
    let mut wire = json!({
        "id": format!("preview-{state}"), "session": "preview", "asker": "subagent",
        "label": "Review Task 3", "agentType": "reviewer", "model": "sonnet",
        "branch": "agent/ask", "question": "The tests pass but the lint fails on two files. Fix the lint now, or leave it for the next task?",
        "choices": [], "mainMayAnswer": false, "state": "open", "waiting": true,
        "createdMs": now_ms - 240_000, "updatedMs": now_ms - 240_000,
    });
    let close = |wire: &mut Value, state: &str| {
        wire["state"] = json!(state);
        wire["waiting"] = json!(false);
        wire["closedMs"] = json!(now_ms - 30_000);
    };
    match state {
        "open" => {
            wire["asker"] = json!("main");
            wire["label"] = Value::Null;
        }
        "open-choices" => wire["choices"] = json!(["Keep it", "Drop it", "Ask me later"]),
        "unknown" => {
            wire["asker"] = json!("unknown");
            wire["label"] = Value::Null;
        }
        "answered" => {
            wire["choices"] = json!(["Keep it", "Drop it"]);
            close(&mut wire, "answered");
            wire["by"] = json!("mac");
            wire["choice"] = json!(1);
            wire["answer"] = json!("Keep it");
        }
        "dismissed" => close(&mut wire, "dismissed"),
        "gone" => close(&mut wire, "gone"),
        _ => return None,
    }
    AskRow::from_wire(&wire)
}

pub fn preview(state: &str, now_ms: i64) -> Option<Vec<AskRow>> {
    row(state, now_ms).map(|row| vec![row])
}

#[cfg(test)]
#[path = "ask_preview_tests.rs"]
mod tests;
