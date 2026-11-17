use crate::acting_ops::{ActingOps, Room};
use crate::ask_ops::AskOps;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::vault_caller::SystemProcesses;
use notchling_engine::ask::AskStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub fn build(
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
    acting: &Arc<ActingOps>,
    room: &Arc<dyn Room>,
    events: &Events,
    inbox: &Arc<InboxOps>,
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
    );
    let ops = Arc::new(if saving { ops } else { ops.without_store() });
    ops.attach(inbox.clone());
    ops
}
