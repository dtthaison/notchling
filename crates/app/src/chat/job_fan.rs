use std::collections::{HashMap, HashSet};
use std::path::Path;

use notchling_engine::contract::Tool;
use notchling_engine::sessions::job_fan::{FanEntry, FanKind};
use notchling_engine::sessions::task_output::{self, Found};
use notchling_engine::transcript::model::{BackgroundShell, Conversation};
use notchling_engine::transcript::subagents::{Status, Subagent};

const LAUNCH_GRACE_S: f64 = 60.0;
const RETRY_MS: i64 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Unknown,
    At(String),
    Gone,
}

#[derive(Default)]
pub struct FanOutputs {
    found: HashMap<String, String>,
    asked: HashMap<String, (i64, Output)>,
}

impl FanOutputs {
    pub fn lookup(&mut self, tmp: &Path, entry: &FanEntry, now_ms: i64) -> Output {
        if let Some(path) = self.found.get(&entry.id) {
            return Output::At(path.clone());
        }
        let Some(started) = entry.started else {
            return Output::Unknown;
        };
        if now_ms as f64 / 1000.0 - started < LAUNCH_GRACE_S {
            return Output::Unknown;
        }
        if let Some((at, answer)) = self.asked.get(&entry.id) {
            if now_ms - at < RETRY_MS {
                return answer.clone();
            }
        }
        let answer = match task_output::find(tmp, &entry.id) {
            Found::At(path) => {
                let path = path.to_string_lossy().into_owned();
                self.asked.remove(&entry.id);
                self.found.insert(entry.id.clone(), path.clone());
                return Output::At(path);
            }
            Found::Missing => Output::Gone,
            Found::Unsearchable => Output::Unknown,
        };
        self.asked
            .insert(entry.id.clone(), (now_ms, answer.clone()));
        answer
    }

    pub fn keep_only(&mut self, fan: &[FanEntry]) {
        let live: HashSet<&str> = fan.iter().map(|e| e.id.as_str()).collect();
        self.found.retain(|id, _| live.contains(id.as_str()));
        self.asked.retain(|id, _| live.contains(id.as_str()));
    }
}

pub fn extra_shells(fan: &[FanEntry], conversation: &Conversation) -> Vec<BackgroundShell> {
    extra_shells_found(fan, conversation, &mut |_| Output::Unknown)
}

pub fn extra_shells_found(
    fan: &[FanEntry],
    conversation: &Conversation,
    output: &mut dyn FnMut(&FanEntry) -> Output,
) -> Vec<BackgroundShell> {
    let known: HashSet<&str> = conversation.shells.iter().map(|s| s.id.as_str()).collect();
    fan.iter()
        .filter(|e| e.kind == FanKind::Shell && !known.contains(e.id.as_str()))
        .filter_map(|e| {
            let output_path = match output(e) {
                Output::Unknown => String::new(),
                Output::At(path) => path,
                Output::Gone => return None,
            };
            Some(BackgroundShell {
                id: e.id.clone(),
                command: e.label.clone(),
                title: String::new(),
                output_path,
                started: e.started,
            })
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
