use serde_json::{json, Value};

use crate::transcript::model::{ChatItem, Conversation, ItemKind, ToolCall, ToolKind};
use crate::transcript::subagents::RESUMED;

mod commands;
mod shell;

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod resumed_tests;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod stopped_tests;

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
        "Agent" | "Task" => true,
        "Artifact" | "ArtifactData" | "ArtifactComments" => artifact_outward(input),
        _ => mcp_outward(tool),
    }
}

fn mcp_outward(tool: &str) -> bool {
    let lower = tool.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("mcp__") else {
        return false;
    };
    let Some((server, _)) = rest.split_once("__") else {
        return true;
    };
    let name = &tool[5 + server.len() + 2..];
    let name = match name.get(..server.len() + 1) {
        Some(head)
            if head
                .strip_suffix('_')
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(server)) =>
        {
            &name[server.len() + 1..]
        }
        _ => name,
    };
    !MCP_READS.iter().any(|read| reads_as(name, read))
}

fn reads_as(name: &str, read: &str) -> bool {
    if !name.is_char_boundary(read.len()) || !name[..read.len()].eq_ignore_ascii_case(read) {
        return false;
    }
    let verb = &name[..read.len()];
    let after = &name[read.len()..];
    match after.chars().next() {
        None => true,
        Some('-') => true,
        Some('_') => !after.starts_with("__"),
        Some(c) => c.is_ascii_uppercase() && verb.chars().all(|v| v.is_ascii_lowercase()),
    }
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
    let mut steps: Vec<CutStep> = calls_in(&conv.items[start..])
        .filter(|call| !call.finished)
        .map(|call| CutStep {
            tool: call.name.clone(),
            summary: call.title.clone(),
            outward: call_outward(call),
        })
        .collect();
    steps.extend(
        conv.shells
            .iter()
            .filter(|shell| !conv.finished_agents.contains_key(&shell.id))
            .map(|shell| CutStep {
                tool: "Bash".to_string(),
                summary: shell.title.clone(),
                outward: shell::is_outward(&shell.command),
            }),
    );
    steps.extend(
        calls_in(&conv.items)
            .filter(|call| call.kind == ToolKind::Agent && call.finished && !call.is_error)
            .filter(|call| !agent_reported(conv, call))
            .map(|call| CutStep {
                tool: call.name.clone(),
                summary: call.title.clone(),
                outward: true,
            }),
    );
    steps
}

fn calls_in(items: &[ChatItem]) -> impl Iterator<Item = &ToolCall> {
    items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Tools(calls) => Some(calls),
            _ => None,
        })
        .flatten()
}

fn call_outward(call: &ToolCall) -> bool {
    if call.kind == ToolKind::Shell {
        return shell::is_outward(&call.input);
    }
    let input = serde_json::from_str(&call.input).unwrap_or_else(|_| json!({}));
    call.kind == ToolKind::Agent || is_outward(&call.name, &input)
}

fn agent_reported(conv: &Conversation, call: &ToolCall) -> bool {
    conv.finished_agents.contains_key(&call.id)
        || call.output.as_deref().is_some_and(|output| {
            conv.finished_agents.iter().any(|(id, word)| {
                word != RESUMED && id.len() >= MIN_AGENT_ID && output.contains(id.as_str())
            })
        })
}
