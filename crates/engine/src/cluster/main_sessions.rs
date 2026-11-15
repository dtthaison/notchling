use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::id::Tool;
use crate::sessions::Agent;

pub const DAY_MS: i64 = 86_400_000;

pub const NOT_MAIN: &str = "Only Claude Code and Codex sessions on the primary can move";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MainSession {
    pub id: String,
    pub tool: Tool,
    pub cwd: String,
    pub config_dir: Option<String>,
    pub transcript: Option<String>,
    pub respawn: Vec<String>,
    pub since_ms: i64,
}

pub fn tool_of_row(id: &str) -> Tool {
    if id.starts_with("codex:") {
        Tool::Codex
    } else if id.starts_with("agy:") || id.starts_with("antigravity:") {
        Tool::Agy
    } else if id.starts_with("shell:") {
        Tool::Shell
    } else {
        Tool::Claude
    }
}

pub fn may_be_main(agent: &Agent) -> Result<(), String> {
    let tool = tool_of_row(&agent.id);
    let movable_tool = matches!(tool, Tool::Claude | Tool::Codex);
    let here = agent.node.is_none() && agent.command.is_none();
    if movable_tool && here && !agent.cwd.trim().is_empty() {
        Ok(())
    } else {
        Err(NOT_MAIN.to_string())
    }
}

pub fn is_main(agent: &Agent, kept: &BTreeSet<String>) -> bool {
    may_be_main(agent).is_ok() && !kept.contains(&agent.id)
}

fn member(agent: &Agent, now_ms: i64) -> MainSession {
    MainSession {
        id: agent.id.clone(),
        tool: tool_of_row(&agent.id),
        cwd: agent.cwd.clone(),
        config_dir: agent.config_dir.clone(),
        transcript: agent.transcript_path.clone(),
        respawn: Vec::new(),
        since_ms: now_ms,
    }
}

pub fn reconcile(
    main: &BTreeMap<String, MainSession>,
    ended: &BTreeMap<String, i64>,
    agents: &[Agent],
    kept: &BTreeSet<String>,
    now_ms: i64,
) -> (BTreeMap<String, MainSession>, BTreeMap<String, i64>) {
    let mut next = BTreeMap::new();
    let mut gone = BTreeMap::new();
    for agent in agents.iter().filter(|a| is_main(a, kept)) {
        let mut entry = member(agent, now_ms);
        if let Some(old) = main.get(&agent.id) {
            entry.since_ms = old.since_ms;
        }
        next.insert(agent.id.clone(), entry);
    }
    for (id, old) in main {
        if next.contains_key(id) || kept.contains(id) {
            continue;
        }
        let since = ended.get(id).copied().unwrap_or(now_ms);
        if now_ms - since < DAY_MS {
            next.insert(id.clone(), old.clone());
            gone.insert(id.clone(), since);
        }
    }
    (next, gone)
}
