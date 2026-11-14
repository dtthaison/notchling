use std::collections::BTreeMap;

use super::{Tool, Version};

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

pub fn mismatch_between(
    mac: &BTreeMap<Tool, Version>,
    machine: &str,
    worker: &BTreeMap<Tool, Version>,
) -> Vec<Mismatch> {
    mac.iter()
        .filter_map(|(tool, mac_version)| {
            let worker_version = worker.get(tool)?;
            (worker_version != mac_version).then(|| Mismatch {
                tool: *tool,
                machine: machine.to_string(),
                worker: worker_version.clone(),
                mac: mac_version.clone(),
            })
        })
        .collect()
}
