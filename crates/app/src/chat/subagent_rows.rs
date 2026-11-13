use std::collections::HashMap;
use std::time::SystemTime;

use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::{
    self, Origin as SubagentOrigin, Status as SubagentStatus, Subagent,
};
use notchling_kit::ui::ToolMark;
use notchling_kit::{Icon, Ink, Status};

use super::format::tokens;
use super::inspector::{Mark, SubagentRow};
use super::paging::Item;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub context: Option<u64>,
    pub doing: Option<String>,
}

impl Facts {
    pub fn of_conversation(conversation: &Conversation) -> Facts {
        Facts {
            context: Some(conversation.facts.context_tokens),
            doing: None,
        }
    }
}

pub fn subagent_row(agent: &Subagent, read: Option<&Facts>) -> SubagentRow {
    let (status, word, mark) = match agent.status {
        SubagentStatus::Running => (Status::Working, "Running", Mark::Spinner),
        SubagentStatus::Completed => (Status::Done, "Done", Mark::Icon(Icon::Done, Ink::Done)),
        SubagentStatus::Failed => (
            Status::Needs,
            "Failed",
            Mark::Icon(Icon::Failed, Ink::Expired),
        ),
        SubagentStatus::Stopped => (
            Status::Needs,
            "Stopped",
            Mark::Icon(Icon::Failed, Ink::TextFaint),
        ),
        SubagentStatus::Quiet => (
            Status::Done,
            "Not running",
            Mark::Icon(Icon::Clock, Ink::TextFaint),
        ),
    };
    let model = [agent.model.as_deref(), agent.effort.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let kind = (agent.agent_type != "general-purpose" && !agent.agent_type.is_empty())
        .then_some(agent.agent_type.as_str());
    let handed_over = agent.origin != SubagentOrigin::Claude;
    let parts = if handed_over {
        [
            Some(word),
            Some(agent.agent_type.as_str()).filter(|t| !t.is_empty()),
            (!model.is_empty()).then_some(model.as_str()),
        ]
    } else {
        [
            Some(word),
            (!model.is_empty()).then_some(model.as_str()),
            kind,
        ]
    };
    let context = read.and_then(|f| f.context).filter(|n| *n > 0).map(tokens);
    let detail = parts
        .into_iter()
        .flatten()
        .chain(context.as_deref())
        .collect::<Vec<_>>()
        .join(" · ");
    SubagentRow {
        id: agent.id.clone(),
        description: agent.description.clone(),
        detail,
        status,
        status_word: word.to_owned(),
        running: agent.is_running(),
        mark,
        tool: match agent.origin {
            SubagentOrigin::Claude => ToolMark::Claude,
            SubagentOrigin::Codex => ToolMark::Codex,
            SubagentOrigin::Antigravity => ToolMark::Antigravity,
        },
        doing: read
            .filter(|_| agent.is_running())
            .and_then(|f| f.doing.clone()),
    }
}

impl Item for Subagent {
    fn id(&self) -> &str {
        &self.id
    }

    fn running(&self) -> bool {
        self.is_running()
    }
}

pub trait Peek {
    fn read(&self, agent: &Subagent) -> Facts;
}

pub struct Disk;

impl Peek for Disk {
    fn read(&self, agent: &Subagent) -> Facts {
        let (origin, path) = (agent.origin, agent.path.as_str());
        Facts {
            context: subagents::context_tokens(origin, path),
            doing: agent
                .is_running()
                .then(|| subagents::recent(origin, path))
                .flatten()
                .map(|c| super::activity::doing(&c)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp_of(path: &str) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

struct Kept {
    stamp: Stamp,
    running: bool,
    facts: Facts,
}

pub struct SubagentRows<P: Peek = Disk> {
    peek: P,
    kept: HashMap<String, Kept>,
}

impl Default for SubagentRows<Disk> {
    fn default() -> Self {
        SubagentRows::new(Disk)
    }
}

impl<P: Peek> SubagentRows<P> {
    pub fn new(peek: P) -> Self {
        SubagentRows {
            peek,
            kept: HashMap::new(),
        }
    }

    pub fn rows(&mut self, agents: &[Subagent], shown: &[usize]) -> Vec<SubagentRow> {
        let rows = shown
            .iter()
            .filter_map(|&i| agents.get(i))
            .map(|agent| {
                let facts = self.facts(agent);
                subagent_row(agent, facts.as_ref())
            })
            .collect();
        self.kept
            .retain(|path, _| agents.iter().any(|a| a.path == *path));
        rows
    }

    fn facts(&mut self, agent: &Subagent) -> Option<Facts> {
        if agent.path.is_empty() || agent.origin == SubagentOrigin::Antigravity {
            return None;
        }
        let stamp = stamp_of(&agent.path)?;
        let running = agent.is_running();
        if let Some(kept) = self.kept.get(&agent.path) {
            if kept.stamp == stamp && (kept.running || !running) {
                return Some(kept.facts.clone());
            }
        }
        let facts = self.peek.read(agent);
        self.kept.insert(
            agent.path.clone(),
            Kept {
                stamp,
                running,
                facts: facts.clone(),
            },
        );
        Some(facts)
    }
}

#[cfg(test)]
#[path = "subagent_rows_tests.rs"]
mod tests;
