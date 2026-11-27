use super::worker::{ToolState, WorkerTools};
use super::{Tool, Version};
use std::collections::{BTreeMap, BTreeSet};

pub type Key = (String, Tool, String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drift {
    pub machine: String,
    pub tool: Tool,
    pub expected: Version,
    pub found: Version,
    pub ahead: bool,
}

impl Drift {
    pub fn key(&self) -> Key {
        (
            self.machine.clone(),
            self.tool,
            self.found.as_str().to_string(),
        )
    }

    pub fn title(&self) -> String {
        format!("{} Runs Another {}", self.machine, self.tool.title())
    }

    pub fn detail(&self) -> String {
        format!(
            "{} runs {} {}, the Mac {}.",
            self.machine,
            self.tool.title(),
            self.found,
            self.expected
        )
    }
}

pub fn drift(
    reference: &BTreeMap<Tool, Version>,
    workers: &BTreeMap<String, WorkerTools>,
    held: &BTreeSet<(String, Tool)>,
    rolling: &BTreeSet<(String, Tool)>,
) -> Vec<Drift> {
    let mut out = Vec::new();
    for (machine, worker) in workers {
        for (tool, state) in &worker.tools {
            let ToolState::Installed {
                version: Some(found),
                ..
            } = state
            else {
                continue;
            };
            let Some(expected) = reference.get(tool) else {
                continue;
            };
            let pair = (machine.clone(), *tool);
            if found == expected || held.contains(&pair) || rolling.contains(&pair) {
                continue;
            }
            out.push(Drift {
                machine: machine.clone(),
                tool: *tool,
                expected: expected.clone(),
                found: found.clone(),
                ahead: found > expected,
            });
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub raise: Vec<Drift>,
    pub withdraw: Vec<Key>,
    pub noticed: BTreeSet<Key>,
}

pub fn plan_notices(now: &[Drift], noticed: &BTreeSet<Key>) -> Plan {
    let current: BTreeSet<Key> = now.iter().map(Drift::key).collect();
    Plan {
        raise: now
            .iter()
            .filter(|d| !noticed.contains(&d.key()))
            .cloned()
            .collect(),
        withdraw: noticed.difference(&current).cloned().collect(),
        noticed: current,
    }
}
