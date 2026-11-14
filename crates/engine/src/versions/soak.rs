use super::{Tool, Version};
use crate::store;
use crate::transcript::strict::Skips;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const SOAK_DOC: &str = "versions/soak";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Soak {
    pub since: Version,
    pub turns: u32,
    pub unknown: u32,
}

impl Soak {
    pub fn new(since: Version) -> Soak {
        Soak {
            since,
            turns: 0,
            unknown: 0,
        }
    }

    pub fn observe(&mut self, skips: &Skips) {
        self.turns = self.turns.max(skips.turns_in(self.since.as_str()));
        self.unknown = self.unknown.max(skips.unknown_in(self.since.as_str()));
    }

    pub fn line(&self) -> String {
        let turns = match self.turns {
            1 => "1 turn read".to_string(),
            n => format!("{n} turns read"),
        };
        let unknown = match self.unknown {
            0 => "nothing unknown".to_string(),
            1 => "1 unknown line".to_string(),
            n => format!("{n} unknown lines"),
        };
        format!("Since {}: {turns}, {unknown}", self.since)
    }

    pub fn red(&self) -> bool {
        self.unknown > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Soaks {
    by_tool: BTreeMap<Tool, Soak>,
}

impl Soaks {
    pub fn new() -> Soaks {
        Soaks::default()
    }

    pub fn get(&self, tool: Tool) -> Option<&Soak> {
        self.by_tool.get(&tool)
    }

    pub fn follow(&mut self, tool: Tool, reference: Option<&Version>) -> Option<&mut Soak> {
        let Some(reference) = reference else {
            self.by_tool.remove(&tool);
            return None;
        };
        let restart = self
            .by_tool
            .get(&tool)
            .is_none_or(|s| s.since != *reference);
        if restart {
            self.by_tool.insert(tool, Soak::new(reference.clone()));
        }
        self.by_tool.get_mut(&tool)
    }

    pub fn load(data: &Path) -> Soaks {
        let parsed: Option<BTreeMap<String, Soak>> = store::doc(data, SOAK_DOC)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok());
        let mut soaks = Soaks::new();
        for (name, soak) in parsed.unwrap_or_default() {
            if let Some(tool) = Tool::from_name(&name) {
                soaks.by_tool.insert(tool, soak);
            }
        }
        soaks
    }

    pub fn save(&self, data: &Path) -> Result<(), String> {
        let map: BTreeMap<&str, &Soak> = self.by_tool.iter().map(|(t, s)| (t.name(), s)).collect();
        store::put_doc(
            data,
            SOAK_DOC,
            &serde_json::to_string(&map).map_err(|e| e.to_string())?,
        )
    }
}
