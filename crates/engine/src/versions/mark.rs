use super::worker::ToolState;
use super::{can_pin, Tool, Version};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Same,
    Behind,
    Ahead,
    Held,
    CantPin,
    NotInstalled,
    Unknown,
}

pub fn mark(tool: Tool, mac: Option<&Version>, worker: &ToolState, held: bool) -> Mark {
    let (version, method) = match worker {
        ToolState::NotInstalled => return Mark::NotInstalled,
        ToolState::Installed {
            version, method, ..
        } => (version.as_ref(), *method),
    };
    if held {
        return Mark::Held;
    }
    if !can_pin(tool, method) {
        return Mark::CantPin;
    }
    match (mac, version) {
        (Some(mac), Some(worker)) => match worker.cmp(mac) {
            Ordering::Equal => Mark::Same,
            Ordering::Less => Mark::Behind,
            Ordering::Greater => Mark::Ahead,
        },
        _ => Mark::Unknown,
    }
}
