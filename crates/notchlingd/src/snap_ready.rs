use crate::cluster_main::is_home;
use crate::cluster_node::ClusterNode;
use crate::snap_send::SnapSender;
use notchling_engine::cluster::ready::{assess, ready_in};
use notchling_engine::cluster::snapshot::Trigger;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::{LocalCmd, NodeId};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

pub type Ready = Arc<dyn Fn(&NodeId) -> bool + Send + Sync>;
pub type SenderSlot = Arc<OnceLock<Weak<SnapSender>>>;

pub struct LastSet {
    node: Arc<OnceLock<Arc<ClusterNode>>>,
    sender: SenderSlot,
    cluster: PathBuf,
}

pub fn standby_set_bytes(cluster: &Path, home: &NodeId) -> u64 {
    StandbyStore::new(cluster, home)
        .current()
        .map_or(0, |m| m.parts.iter().map(|p| p.len).sum())
}

impl LastSet {
    pub fn new(
        node: Arc<OnceLock<Arc<ClusterNode>>>,
        sender: SenderSlot,
        cluster: PathBuf,
    ) -> LastSet {
        LastSet {
            node,
            sender,
            cluster,
        }
    }

    pub fn mb(&self) -> u64 {
        let Some(node) = self.node.get() else {
            return 0;
        };
        let status = node.status();
        let bytes = if is_home(&status) {
            self.sender
                .get()
                .and_then(Weak::upgrade)
                .map_or(0, |s| s.last_set_bytes())
        } else {
            status
                .get("cluster")
                .and_then(|c| c.get("home"))
                .and_then(Value::as_str)
                .and_then(NodeId::parse)
                .map_or(0, |home| standby_set_bytes(&self.cluster, &home))
        };
        bytes.div_ceil(1024 * 1024)
    }
}

pub fn predicate(last: Arc<LastSet>) -> Ready {
    Arc::new(move |id: &NodeId| {
        let Some(node) = last.node.get() else {
            return false;
        };
        ready_in(&node.status(), id, last.mb())
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
