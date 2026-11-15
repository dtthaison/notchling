use crate::cluster_main::is_home;
use crate::cluster_node::ClusterNode;
use crate::snap_send::SnapSender;
use notchling_engine::cluster::ready::{assess, ready_in};
use notchling_engine::cluster::snapshot::Trigger;
use notchling_engine::cluster::{LocalCmd, NodeId};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

pub type Ready = Arc<dyn Fn(&NodeId) -> bool + Send + Sync>;
pub type SenderSlot = Arc<OnceLock<Arc<SnapSender>>>;

pub fn last_set_mb_of(sender: &SenderSlot) -> u64 {
    sender.get().map_or(0, |s| s.last_set_mb())
}

pub fn predicate(node: Arc<OnceLock<Arc<ClusterNode>>>, sender: SenderSlot) -> Ready {
    Arc::new(move |id: &NodeId| {
        let Some(node) = node.get() else {
            return false;
        };
        ready_in(&node.status(), id, last_set_mb_of(&sender))
    })
}

pub fn ready_set(status: &Value, last_set_mb: u64) -> BTreeSet<NodeId> {
    let me = status
        .get("me")
        .and_then(Value::as_str)
        .and_then(NodeId::parse);
    assess(status, last_set_mb)
        .into_iter()
        .filter(|(id, a)| a.ready.is_ok() && Some(id) != me.as_ref())
        .map(|(id, _)| id)
        .collect()
}

#[derive(Default)]
pub struct ReadyWatch {
    known: BTreeSet<NodeId>,
}

impl ReadyWatch {
    pub fn newly_ready(&mut self, status: &Value, last_set_mb: u64) -> Vec<NodeId> {
        if !is_home(status) {
            self.known.clear();
            return Vec::new();
        }
        let now = ready_set(status, last_set_mb);
        let fresh = now.difference(&self.known).cloned().collect();
        self.known = now;
        fresh
    }

    pub fn forget(&mut self, id: &NodeId) {
        self.known.remove(id);
    }
}

pub fn ask_for(node: &ClusterNode, fresh: &[NodeId]) -> Vec<NodeId> {
    fresh
        .iter()
        .filter(|id| {
            node.local(LocalCmd::Snapshot {
                trigger: Trigger::Ready((*id).clone()),
            })
            .is_err()
        })
        .cloned()
        .collect()
}

pub fn watch_step(node: &ClusterNode, sender: &SnapSender, watch: &mut ReadyWatch) -> usize {
    let fresh = watch.newly_ready(&node.status(), sender.last_set_mb());
    let refused = ask_for(node, &fresh);
    for id in &refused {
        watch.forget(id);
    }
    if !fresh.is_empty() {
        sender.tick();
    }
    fresh.len() - refused.len()
}

#[cfg(test)]
#[path = "snap_ready_tests.rs"]
mod tests;
