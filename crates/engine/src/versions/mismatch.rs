use super::worker::{ToolState, WorkerTools};
use super::{Tool, Version};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub tool: Tool,
    pub machine: String,
    pub worker: Version,
    pub mac: Version,
}

impl Mismatch {
    pub fn text(&self) -> String {
        format!(
            "{} runs {} {}, the Mac {}",
            self.machine,
            self.tool.title(),
            self.worker,
            self.mac
        )
    }
}

pub fn mismatch(
    mac: &BTreeMap<Tool, Version>,
    machine: &str,
    worker: &WorkerTools,
) -> Vec<Mismatch> {
    let mut out = Vec::new();
    for (tool, state) in &worker.tools {
        let ToolState::Installed {
            version: Some(have),
            ..
        } = state
        else {
            continue;
        };
        let Some(want) = mac.get(tool) else {
            continue;
        };
        if have != want {
            out.push(Mismatch {
                tool: *tool,
                machine: machine.to_string(),
                worker: have.clone(),
                mac: want.clone(),
            });
        }
    }
    out
}
