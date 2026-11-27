use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::id::Tool;
use crate::sessions::{Agent, AgentTarget};

pub const DAY_MS: i64 = 86_400_000;

pub const NOT_MAIN: &str = "Only Claude Code and Codex sessions on the primary can move";
pub const NOT_YET: &str = "That session has not started yet";
pub const HEADLESS: &str = "A claude -p session can't move";
pub const EXITED: &str = "That session has ended";
pub const MAX_ID: usize = 256;

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && !id.chars().any(char::is_control)
}

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
    if !(movable_tool && here && !agent.cwd.trim().is_empty()) {
        return Err(NOT_MAIN.to_string());
    }
    if agent.headless {
        return Err(HEADLESS.to_string());
    }
    if tool == Tool::Claude && matches!(agent.target, AgentTarget::Resume { .. }) {
        return Err(EXITED.to_string());
    }
    if let AgentTarget::Job(short) = &agent.target {
        if agent.id == *short || agent.transcript_path.is_none() {
            return Err(NOT_YET.to_string());
        }
    }
    Ok(())
}

pub fn is_main(agent: &Agent, kept: &BTreeSet<String>) -> bool {
    if may_be_main(agent).is_err() || kept.contains(&agent.id) {
        return false;
    }
    match &agent.target {
        AgentTarget::Job(short) => !kept.contains(short),
        _ => true,
    }
}

pub fn aliases(agents: &[Agent]) -> BTreeMap<String, String> {
    agents
        .iter()
        .filter_map(|a| match &a.target {
            AgentTarget::Job(short) if *short != a.id && !short.is_empty() => {
                Some((short.clone(), a.id.clone()))
            }
            _ => None,
        })
        .collect()
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

pub type Reconciled = (
    BTreeMap<String, MainSession>,
    BTreeMap<String, i64>,
    BTreeSet<String>,
);

pub fn reconcile(
    main: &BTreeMap<String, MainSession>,
    ended: &BTreeMap<String, i64>,
    agents: &[Agent],
    (kept, codex_exec): (&BTreeSet<String>, &BTreeSet<String>),
    now_ms: i64,
) -> Reconciled {
    let alias = aliases(agents);
    let kept: BTreeSet<String> = kept
        .iter()
        .map(|k| alias.get(k).unwrap_or(k).clone())
        .collect();
    let kept = &kept;
    let mut next = BTreeMap::new();
    let mut gone = BTreeMap::new();
    let wanted = |a: &&Agent| {
        is_main(a, kept) && (tool_of_row(&a.id) != Tool::Codex || codex_exec.contains(&a.id))
    };
    for agent in agents.iter().filter(wanted) {
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
    (next, gone, kept.clone())
}
