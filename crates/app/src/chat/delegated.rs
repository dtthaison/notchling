use std::path::Path;
use std::sync::Arc;

use notchling_engine::contract::flows::delegate_jobs;
use notchling_engine::contract::subagents::{Roots, SessionRef};
use notchling_engine::contract::Tool;
use notchling_engine::delegate::record::Delegation;
use notchling_engine::transcript::model::Timestamp;
use notchling_engine::transcript::subagents::Subagent;
use notchling_locale as t;

pub use notchling_engine::contract::flows::delegate_jobs::{folder, hidden, parent_ids, read};

pub struct Places<'a> {
    pub codex_sessions: &'a Path,
    pub antigravity: &'a Path,
}

fn roots(places: &Places<'_>) -> Roots {
    Roots {
        codex_sessions: places.codex_sessions.to_path_buf(),
        antigravity: places.antigravity.to_path_buf(),
        ..Roots::default()
    }
}

fn parent_of(session: &str) -> SessionRef {
    SessionRef {
        id: session.to_owned(),
        tool: Tool::Claude,
        transcript: None,
        name: String::new(),
        finished: Arc::default(),
        session: session.to_owned(),
    }
}

pub fn subagents(
    records: &[Delegation],
    parents: &[String],
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Vec<Subagent> {
    let parent = parent_of(parents.first().map_or("", String::as_str));
    delegate_jobs::select(records, parents, &parent, &roots(places), now, alive)
        .into_iter()
        .map(retitled)
        .collect()
}

fn retitled(mut agent: Subagent) -> Subagent {
    agent.agent_type = match agent.origin.tool {
        Tool::Antigravity => t::chat::delegated_antigravity(),
        _ => t::chat::delegated_codex(),
    }
    .to_owned();
    agent
}

pub fn subagent(
    record: &Delegation,
    places: &Places<'_>,
    now: Timestamp,
    alive: &dyn Fn(u32) -> bool,
) -> Subagent {
    let parent = parent_of(&record.parent_session);
    retitled(delegate_jobs::subagent(
        record,
        &parent,
        &roots(places),
        now,
        alive,
    ))
}

#[cfg(test)]
#[path = "delegated_tests.rs"]
mod tests;
