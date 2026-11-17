use std::collections::HashSet;

use notchling_engine::contract::Tool;
use notchling_engine::sessions::job_fan::{FanEntry, FanKind};
use notchling_engine::transcript::model::{BackgroundShell, Conversation};
use notchling_engine::transcript::subagents::{Status, Subagent};

pub fn extra_shells(fan: &[FanEntry], conversation: &Conversation) -> Vec<BackgroundShell> {
    let known: HashSet<&str> = conversation.shells.iter().map(|s| s.id.as_str()).collect();
    fan.iter()
        .filter(|e| e.kind == FanKind::Shell && !known.contains(e.id.as_str()))
        .map(|e| BackgroundShell {
            id: e.id.clone(),
            command: e.label.clone(),
            title: String::new(),
            output_path: String::new(),
            started: e.started,
        })
        .collect()
}

pub fn mark_running(subagents: &mut [Subagent], fan: &[FanEntry]) {
    let flying: HashSet<&str> = fan
        .iter()
        .filter(|e| e.kind == FanKind::Agent)
        .map(|e| e.id.as_str())
        .collect();
    if flying.is_empty() {
        return;
    }
    for agent in subagents
        .iter_mut()
        .filter(|a| a.origin.tool == Tool::Claude && a.status != Status::Running)
    {
        if flying.contains(agent.own_id()) {
            agent.status = Status::Running;
        }
    }
}

pub fn merged<'a>(
    conversation: &'a Conversation,
    extra: &'a [BackgroundShell],
) -> Vec<(&'a BackgroundShell, bool)> {
    let mut all: Vec<(&BackgroundShell, bool)> =
        conversation.shells.iter().map(|s| (s, false)).collect();
    for shell in extra {
        let at = shell.started.and_then(|started| {
            all.iter()
                .position(|(s, _)| s.started.is_some_and(|other| other > started))
        });
        all.insert(at.unwrap_or(all.len()), (shell, true));
    }
    all
}

#[cfg(test)]
#[path = "job_fan_tests.rs"]
mod tests;
