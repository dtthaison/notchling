use serde_json::{json, Value};

use crate::transcript::model::{Conversation, ItemKind, ToolCall, ToolKind};

mod commands;
mod shell;

const MCP_READS: [&str; 8] = [
    "get", "list", "search", "read", "fetch", "find", "view", "describe",
];

const READ_ACTIONS: [&str; 5] = ["read", "get", "list", "view", "query"];

const MIN_AGENT_ID: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutStep {
    pub tool: String,
    pub summary: String,
    pub outward: bool,
}

pub fn is_outward(tool: &str, input: &Value) -> bool {
    let command = input.get("command").and_then(Value::as_str);
    match tool {
        "Bash" => command.is_some_and(shell::is_outward),
        "Monitor" => command.is_none_or(shell::is_outward),
        "Artifact" | "ArtifactData" | "ArtifactComments" => artifact_outward(input),
        _ => mcp_outward(tool),
    }
}

fn mcp_outward(tool: &str) -> bool {
    let lower = tool.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("mcp__") else {
        return false;
    };
    let name = rest
        .split_once("__")
        .map_or(lower.as_str(), |(_, name)| name);
    !MCP_READS.iter().any(|read| {
        name.strip_prefix(read)
            .is_some_and(|after| after.is_empty() || after.starts_with('_'))
    })
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
    let calls: Vec<&ToolCall> = conv.items[start..]
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Tools(calls) => Some(calls),
            _ => None,
        })
        .flatten()
        .collect();
    let mut steps: Vec<CutStep> = calls
        .iter()
        .filter(|call| !call.finished)
        .map(|call| CutStep {
            tool: call.name.clone(),
            summary: call.title.clone(),
            outward: call.kind == ToolKind::Agent || is_outward(&call.name, &input_of(call)),
        })
        .collect();
    steps.extend(
        conv.shells
            .iter()
            .filter(|shell| !conv.finished_agents.contains_key(&shell.id))
            .map(|shell| CutStep {
                tool: "Bash".to_string(),
                summary: shell.title.clone(),
                outward: is_outward("Bash", &json!({ "command": shell.command })),
            }),
    );
    steps.extend(
        calls
            .iter()
            .filter(|call| call.kind == ToolKind::Agent && call.finished)
            .filter(|call| !agent_reported(conv, call))
            .map(|call| CutStep {
                tool: call.name.clone(),
                summary: call.title.clone(),
                outward: true,
            }),
    );
    steps
}

fn agent_reported(conv: &Conversation, call: &ToolCall) -> bool {
    conv.finished_agents.contains_key(&call.id)
        || call.output.as_deref().is_some_and(|output| {
            conv.finished_agents
                .keys()
                .any(|id| id.len() >= MIN_AGENT_ID && output.contains(id.as_str()))
        })
}

fn input_of(call: &ToolCall) -> Value {
    if call.kind == ToolKind::Shell {
        return json!({ "command": call.input });
    }
    serde_json::from_str(&call.input).unwrap_or_else(|_| json!({}))
}
