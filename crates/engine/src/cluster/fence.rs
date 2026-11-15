use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::members::Term;

pub const FENCE_FILE: &str = "cluster/fence.json";
pub const MAX_FENCE_BYTES: usize = 65_536;
pub const FENCE_VERSION: u32 = 1;
pub const WAIT_MAX_MS: u64 = 20_000;
pub const POLL_MS: u64 = 250;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FenceFile {
    pub v: u32,
    pub term: Term,
    pub node: NodeId,
    pub boot_id: String,
    pub valid_until: u64,
    pub sessions: BTreeMap<String, Gate>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gate {
    Lease,
    Free,
    Moved { to: String },
    Waiting { for_node: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fence {
    Pass,
    Wait,
    Deny(String),
}

pub fn decide(f: Option<&FenceFile>, session: &str, boot_id: &str, now: u64) -> Fence {
    let Some(f) = f else {
        return Fence::Pass;
    };
    let Some(gate) = f.sessions.get(session) else {
        return Fence::Pass;
    };
    match gate {
        Gate::Free => Fence::Pass,
        Gate::Moved { to } => Fence::Deny(format!("This session continues on {to}. Open it there.")),
        Gate::Waiting { for_node } => Fence::Deny(format!(
            "This session waits for {for_node}, which has newer work in it. To go on here, choose Continue From This Copy in Notchling."
        )),
        Gate::Lease => {
            if !boot_id.is_empty() && f.boot_id == boot_id && now < f.valid_until {
                Fence::Pass
            } else {
                Fence::Wait
            }
        }
    }
}

pub fn after_wait() -> Fence {
    Fence::Deny(
        "Notchling can't reach the Standby that would take over this session, so it's paused to keep it from running on two machines. Check the cluster in Notchling."
            .to_string(),
    )
}

pub fn parse(bytes: &[u8]) -> Option<FenceFile> {
    if bytes.len() > MAX_FENCE_BYTES {
        return None;
    }
    let f: FenceFile = serde_json::from_slice(bytes).ok()?;
    if f.v == 0 || f.v > FENCE_VERSION {
        return None;
    }
    Some(f)
}

pub fn decide_bytes(bytes: Option<&[u8]>, session: &str, boot_id: &str, now: u64) -> Fence {
    match bytes {
        None => Fence::Pass,
        Some(b) => match parse(b) {
            Some(f) => decide(Some(&f), session, boot_id, now),
            None => Fence::Wait,
        },
    }
}
