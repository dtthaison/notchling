use crate::cluster_move::{LateLink, Snaps, SNAP_WAIT};
use crate::snap_run::{Outcome, SnapRunner};
use crate::snap_send::SnapSender;
use notchling_engine::cluster::snapshot::{Manifest, Trigger};
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::{LocalCmd, NodeId};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

pub struct RealSnaps {
    pub node: Arc<LateLink>,
    pub runner: Arc<SnapRunner>,
    pub sender: Arc<SnapSender>,
}

fn asked(node: &LateLink) -> Result<u64, WireError> {
    let answer = node.local(
        LocalCmd::Snapshot {
            trigger: Trigger::Final,
        },
        Duration::from_secs(30),
    )?;
    answer
        .get("asked")
        .and_then(Value::as_u64)
        .ok_or_else(|| WireError::new("failed", "no snapshot was asked for"))
}

impl Snaps for RealSnaps {
    fn latest(&self) -> Option<Manifest> {
        self.sender.latest_manifest()
    }

    fn catch_up(&self, to: &NodeId, name: &str) -> Result<(), String> {
        let Some(latest) = self.sender.latest_manifest() else {
            return Ok(());
        };
        self.sender.tick();
        self.sender.wait_idle(SNAP_WAIT);
        if self.sender.sent(to).as_deref() == Some(latest.set.as_str()) {
            return Ok(());
        }
        let why = self
            .sender
            .last_error(to)
            .map_or_else(String::new, |e| format!(": {e}"));
        Err(format!("{name} hasn't caught up with the snapshots{why}"))
    }

    fn final_set(&self, to: &NodeId, name: &str) -> Result<Manifest, String> {
        let seq = asked(&self.node)
            .map_err(|e| format!("the final snapshot wasn't started: {}", e.message))?;
        self.runner.wait_idle(SNAP_WAIT);
        self.sender.wait_idle(SNAP_WAIT);
        let set = match self.runner.outcome(seq) {
            Some(Outcome::Staged(set)) => set,
            Some(Outcome::NoReadyTarget) => {
                return Err("no standby was ready for the final snapshot".to_string())
            }
            Some(Outcome::Superseded) => {
                return Err("another snapshot replaced the final one".to_string())
            }
            Some(Outcome::Failed(why)) => return Err(format!("the final snapshot failed: {why}")),
            None => return Err("the final snapshot didn't finish".to_string()),
        };
        if self.sender.sent(to).as_deref() != Some(set.as_str()) {
            let why = self
                .sender
                .last_error(to)
                .map_or_else(String::new, |e| format!(": {e}"));
            return Err(format!("{name} didn't get the final snapshot{why}"));
        }
        self.sender
            .latest_manifest()
            .filter(|m| m.set == set)
            .ok_or_else(|| "the final snapshot isn't the newest set".to_string())
    }
}
