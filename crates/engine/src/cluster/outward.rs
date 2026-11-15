use serde_json::{json, Value};

use crate::transcript::model::{Conversation, ItemKind, ToolCall, ToolKind};

mod commands;
mod shell;

const MCP_WORDS: [&str; 10] = [
    "send", "post", "create", "update", "delete", "publish", "deploy", "merge", "write", "reply",
];

const READ_ACTIONS: [&str; 4] = ["read", "get", "list", "view"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutStep {
    pub tool: String,
    pub summary: String,
    pub outward: bool,
}

pub fn is_outward(tool: &str, input: &Value) -> bool {
    if tool == "Bash" {
        return input
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(shell::is_outward);
    }
    if tool == "Artifact" {
        return artifact_outward(input);
    }
    let lower = tool.to_ascii_lowercase();
    lower.starts_with("mcp__") && MCP_WORDS.iter().any(|word| lower.contains(word))
}

fn artifact_outward(input: &Value) -> bool {
    let action = ["action", "mode"]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str));
    match action {
        Some(action) => !READ_ACTIONS.contains(&action.to_ascii_lowercase().as_str()),
        None => true,
    }
}

pub fn cut_steps(conv: &Conversation) -> Vec<CutStep> {
    let start = conv
        .items
        .iter()
        .rposition(|item| matches!(item.kind, ItemKind::User { .. }))
        .map_or(0, |at| at + 1);
    conv.items[start..]
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Tools(calls) => Some(calls),
            _ => None,
        })
        .flatten()
        .filter(|call| !call.finished)
        .map(|call| CutStep {
            tool: call.name.clone(),
            summary: call.title.clone(),
            outward: is_outward(&call.name, &input_of(call)),
        })
        .collect()
}

fn input_of(call: &ToolCall) -> Value {
    if call.kind == ToolKind::Shell {
        return json!({ "command": call.input });
    }
    serde_json::from_str(&call.input).unwrap_or_else(|_| json!({}))
}
