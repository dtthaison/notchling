use super::tail::read_lines;
use super::util::{parse_object, str_of};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_LINE: usize = 400_000;

#[cfg(test)]
#[path = "strict_tests.rs"]
mod tests;

const CLAUDE_LINES: &[&str] = &[
    "system",
    "assistant",
    "user",
    "queue-operation",
    "attachment",
    "summary",
    "file-history-snapshot",
    "file-history-delta",
    "progress",
    "last-prompt",
    "ai-title",
    "custom-title",
    "agent-name",
    "agent-color",
    "permission-mode",
    "mode",
    "pr-link",
    "continued-in",
    "worktree-state",
    "session-owner",
    "relocated",
    "api-request",
    "api-request-blob",
    "api-request-shape",
    "frame-link",
    "tools-used",
    "cost-state",
    "atis-latch",
    "artifact-autoreact-ledger",
    "artifact-comment-monitor",
    "tag",
];

const CLAUDE_BLOCKS: &[&str] = &[
    "text",
    "thinking",
    "redacted_thinking",
    "tool_use",
    "tool_result",
    "image",
    "document",
    "server_tool_use",
    "web_search_tool_result",
    "mcp_tool_use",
    "mcp_tool_result",
];

const QUEUE_OPS: &[&str] = &["enqueue", "dequeue", "remove", "popAll"];

const CODEX_LINES: &[&str] = &[
    "session_meta",
    "turn_context",
    "response_item",
    "event_msg",
    "compacted",
    "token_usage_record",
    "world_state",
    "inter_agent_communication_metadata",
];

const CODEX_ITEMS: &[&str] = &[
    "message",
    "reasoning",
    "function_call",
    "function_call_output",
    "custom_tool_call",
    "custom_tool_call_output",
    "local_shell_call",
    "web_search_call",
    "additional_tools",
    "agent_message",
    "ghost_snapshot",
    "compaction",
];

const CODEX_EVENTS: &[&str] = &[
    "task_started",
    "task_complete",
    "turn_aborted",
    "token_count",
    "error",
    "item_started",
    "item_completed",
    "thread_settings_applied",
    "agent_message",
    "user_message",
    "agent_reasoning",
    "agent_reasoning_raw_content",
    "exec_command_begin",
    "exec_command_end",
    "exec_command_output_delta",
    "patch_apply_begin",
    "patch_apply_end",
    "mcp_tool_call_begin",
    "mcp_tool_call_end",
    "web_search_begin",
    "web_search_end",
    "turn_diff",
    "plan_update",
    "context_compacted",
    "thread_rolled_back",
    "entered_review_mode",
    "exited_review_mode",
    "stream_error",
    "background_event",
    "shutdown_complete",
    "session_configured",
];

const ACP_UPDATES: &[&str] = &[
    "user_message_chunk",
    "agent_message_chunk",
    "agent_thought_chunk",
    "tool_call",
    "tool_call_update",
    "plan",
    "session_info_update",
    "usage_update",
    "available_commands_update",
    "current_mode_update",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    UnknownLine,
    UnknownBlock,
    UnknownQueueOp,
    ParseFailed,
    UnknownAcpUpdate,
    UnknownCodexEvent,
}

impl SkipKind {
    pub fn word(self) -> &'static str {
        match self {
            SkipKind::UnknownLine => "unknown_line",
            SkipKind::UnknownBlock => "unknown_block",
            SkipKind::UnknownQueueOp => "unknown_queue_op",
            SkipKind::ParseFailed => "parse_failed",
            SkipKind::UnknownAcpUpdate => "unknown_acp_update",
            SkipKind::UnknownCodexEvent => "unknown_codex_event",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skips {
    pub by_version: BTreeMap<String, BTreeMap<SkipKind, u32>>,
    pub lines_by_version: BTreeMap<String, u32>,
    pub turns_by_version: BTreeMap<String, u32>,
}

fn key(version: Option<&str>) -> String {
    version.unwrap_or("").to_string()
}

fn bump(slot: &mut u32, by: u32) {
    *slot = slot.saturating_add(by);
}

impl Skips {
    pub fn note(&mut self, version: Option<&str>, kind: SkipKind) {
        let slot = self
            .by_version
            .entry(key(version))
            .or_default()
            .entry(kind)
            .or_insert(0);
        bump(slot, 1);
    }

    pub fn line(&mut self, version: Option<&str>) {
        bump(self.lines_by_version.entry(key(version)).or_insert(0), 1);
    }

    pub fn turn(&mut self, version: Option<&str>) {
        bump(self.turns_by_version.entry(key(version)).or_insert(0), 1);
    }

    pub fn count(&self, version: &str, kind: SkipKind) -> u32 {
        self.by_version
            .get(version)
            .and_then(|m| m.get(&kind))
            .copied()
            .unwrap_or(0)
    }

    pub fn unknown_in(&self, version: &str) -> u32 {
        self.by_version
            .get(version)
            .map_or(0, |m| m.values().fold(0u32, |a, b| a.saturating_add(*b)))
    }

    pub fn unknown_total(&self) -> u32 {
        self.by_version
            .values()
            .flat_map(|m| m.values())
            .fold(0u32, |a, b| a.saturating_add(*b))
    }

    pub fn first_kind_in(&self, version: &str) -> Option<SkipKind> {
        self.by_version
            .get(version)?
            .iter()
            .find(|(_, n)| **n > 0)
            .map(|(k, _)| *k)
    }

    pub fn turns_in(&self, version: &str) -> u32 {
        self.turns_by_version.get(version).copied().unwrap_or(0)
    }

    pub fn merge(&mut self, other: &Skips) {
        for (version, kinds) in &other.by_version {
            for (kind, n) in kinds {
                let slot = self
                    .by_version
                    .entry(version.clone())
                    .or_default()
                    .entry(*kind)
                    .or_insert(0);
                bump(slot, *n);
            }
        }
        for (version, n) in &other.lines_by_version {
            bump(
                self.lines_by_version.entry(version.clone()).or_insert(0),
                *n,
            );
        }
        for (version, n) in &other.turns_by_version {
            bump(
                self.turns_by_version.entry(version.clone()).or_insert(0),
                *n,
            );
        }
    }
}

pub fn strict_claude(path: &Path, budget: u64) -> Skips {
    let mut skips = Skips::default();
    if let Ok(lines) = read_lines(&path.to_string_lossy(), budget) {
        claude_lines(&mut skips, lines.iter().map(|l| l.as_bytes()));
    }
    skips
}

pub fn claude_lines<'a>(skips: &mut Skips, lines: impl Iterator<Item = &'a [u8]>) {
    for line in lines {
        if line.len() > MAX_LINE {
            continue;
        }
        let Some(map) = parse_object(line) else {
            skips.line(None);
            skips.note(None, SkipKind::ParseFailed);
            continue;
        };
        let obj = Value::Object(map);
        let version = str_of(&obj, "version").map(str::to_string);
        let version = version.as_deref();
        skips.line(version);
        let Some(kind) = str_of(&obj, "type") else {
            skips.note(version, SkipKind::UnknownLine);
            continue;
        };
        if !CLAUDE_LINES.contains(&kind) {
            skips.note(version, SkipKind::UnknownLine);
            continue;
        }
        match kind {
            "system" => {
                if str_of(&obj, "subtype") == Some("turn_duration") {
                    skips.turn(version);
                }
            }
            "assistant" | "user" => claude_blocks(skips, version, &obj),
            "queue-operation"
                if !str_of(&obj, "operation").is_some_and(|op| QUEUE_OPS.contains(&op)) =>
            {
                skips.note(version, SkipKind::UnknownQueueOp);
            }
            _ => {}
        }
    }
}

fn claude_blocks(skips: &mut Skips, version: Option<&str>, obj: &Value) {
    let Some(blocks) = obj
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for block in blocks {
        let known = str_of(block, "type").is_some_and(|t| CLAUDE_BLOCKS.contains(&t));
        if !known {
            skips.note(version, SkipKind::UnknownBlock);
        }
    }
}

pub fn strict_codex(path: &Path, budget: u64) -> Skips {
    let mut skips = Skips::default();
    let Ok(lines) = read_lines(&path.to_string_lossy(), budget) else {
        return skips;
    };
    let head = head_version(path);
    codex_lines(
        &mut skips,
        head.as_deref(),
        lines.iter().map(|l| l.as_bytes()),
    );
    skips
}

fn head_version(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(MAX_LINE as u64).read_to_end(&mut buf).ok()?;
    let first = buf.split(|&b| b == b'\n').next()?;
    let obj = Value::Object(parse_object(first)?);
    if str_of(&obj, "type") != Some("session_meta") {
        return None;
    }
    obj.get("payload")
        .and_then(|p| str_of(p, "cli_version"))
        .map(str::to_string)
}

pub fn codex_lines<'a>(
    skips: &mut Skips,
    head: Option<&str>,
    lines: impl Iterator<Item = &'a [u8]>,
) {
    let mut version: Option<String> = head.map(str::to_string);
    for line in lines {
        if line.len() > MAX_LINE {
            continue;
        }
        let Some(map) = parse_object(line) else {
            skips.line(version.as_deref());
            skips.note(version.as_deref(), SkipKind::ParseFailed);
            continue;
        };
        let obj = Value::Object(map);
        let kind = str_of(&obj, "type");
        let payload = obj.get("payload");
        if kind == Some("session_meta") {
            if let Some(v) = payload.and_then(|p| str_of(p, "cli_version")) {
                version = Some(v.to_string());
            }
        }
        let v = version.as_deref();
        skips.line(v);
        let Some(kind) = kind else {
            skips.note(v, SkipKind::UnknownCodexEvent);
            continue;
        };
        if !CODEX_LINES.contains(&kind) {
            skips.note(v, SkipKind::UnknownCodexEvent);
            continue;
        }
        let inner = payload.and_then(|p| str_of(p, "type"));
        let set = match kind {
            "response_item" => Some(CODEX_ITEMS),
            "event_msg" => Some(CODEX_EVENTS),
            _ => None,
        };
        if let Some(set) = set {
            if !inner.is_some_and(|t| set.contains(&t)) {
                skips.note(v, SkipKind::UnknownCodexEvent);
            }
        }
        if kind == "event_msg" && inner == Some("task_complete") {
            skips.turn(v);
        }
    }
}

pub fn strict_acp(lines: &[String]) -> Skips {
    let mut skips = Skips::default();
    for line in lines {
        if line.len() > MAX_LINE {
            continue;
        }
        let Some(map) = parse_object(line.as_bytes()) else {
            skips.line(None);
            skips.note(None, SkipKind::ParseFailed);
            continue;
        };
        let obj = Value::Object(map);
        let version = str_of(&obj, "version").map(str::to_string);
        let version = version.as_deref();
        skips.line(version);
        let update = [obj.get("update"), obj.pointer("/params/update"), Some(&obj)]
            .into_iter()
            .flatten()
            .find(|u| u.get("sessionUpdate").is_some());
        let Some(update) = update else {
            continue;
        };
        if !str_of(update, "sessionUpdate").is_some_and(|k| ACP_UPDATES.contains(&k)) {
            skips.note(version, SkipKind::UnknownAcpUpdate);
        }
    }
    skips
}
