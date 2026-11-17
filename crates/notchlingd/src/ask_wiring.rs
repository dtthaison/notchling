use crate::acting_ops::{ActingOps, Room};
use crate::ask_ops::{AskOps, Worker};
use crate::cluster_node::ClusterNode;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::vault_caller::SystemProcesses;
use notchling_engine::ask::AskStore;
use notchling_engine::cluster::state::role_of;
use notchling_engine::cluster::{ClusterState, NodeId, Role};
use serde_json::Value;
use std::sync::{Arc, Mutex, OnceLock};

pub type Slot = Arc<OnceLock<Arc<ClusterNode>>>;

pub(crate) fn worker_name(state: &ClusterState, me: &NodeId) -> Option<String> {
    let member = state.members.get(me)?;
    (!member.client && role_of(state, me) == Role::Worker).then(|| member.name.clone())
}

pub fn worker_in(slot: Slot) -> Worker {
    Arc::new(move || {
        let node = slot.get()?;
        worker_name(&node.state()?, node.me())
    })
}

pub fn build(
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
    acting: &Arc<ActingOps>,
    room: &Arc<dyn Room>,
    events: &Events,
    inbox: &Arc<InboxOps>,
    worker: Worker,
) -> Arc<AskOps> {
    let (store, saving) = match AskStore::open(&layout.data_dir) {
        Ok(store) => (store, true),
        Err(_) => (AskStore::in_memory_for_tests(), false),
    };
    let ops = AskOps::new(
        Arc::new(store),
        sessions.clone(),
        layout.roots(),
        acting.acting_switch(),
        room.clone(),
        Some(events.clone()),
        Arc::new(SystemProcesses),
    )
    .with_worker(worker);
    let ops = Arc::new(if saving { ops } else { ops.without_store() });
    ops.attach(inbox.clone());
    ops.attach_finder();
    ops
}
