use crate::acting_ops::{ActingOps, Room};
use crate::bridge_backend::APPROVAL_WAIT;
use crate::bridge_ops::BridgeSetup;
use crate::identity::Identifier;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::outbox_ops::OutboxOps;
use crate::shells_ops::ShellsOps;
use notchling_engine::control::session::home_of;
use notchling_engine::outbox::OutboxStore;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(crate) fn bridge_setup(layout: &Layout, ports: (u16, u16)) -> BridgeSetup {
    let beside = |dir: &std::path::Path| dir.parent().unwrap_or(dir).join("AgentConsole");
    let data_dir = beside(&layout.data_dir);
    let images_dir = match (layout.fixture, notchling_platform::paths::cache_dir()) {
        (false, Some(cache)) => beside(&cache).join("phone-images"),
        _ => data_dir.join("phone-images"),
    };
    BridgeSetup {
        data_dir,
        home: PathBuf::from(home_of(&layout.roots())),
        images_dir,
        projects_dir: layout.claude_dir.join("projects"),
        lan_port: ports.0,
        tunnel_port: ports.1,
        loopback_only: false,
        approval_wait: APPROVAL_WAIT,
    }
}

pub(crate) fn outbox_ops(
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
    acting: &Arc<ActingOps>,
    room: &Arc<dyn Room>,
    events: &Events,
) -> OutboxOps {
    let (store, saving) = match OutboxStore::open(&layout.data_dir) {
        Ok(store) => (store, true),
        Err(_) => (OutboxStore::in_memory_for_tests(), false),
    };
    let ops = OutboxOps::new(
        Arc::new(store),
        sessions.clone(),
        layout.roots(),
        acting.acting_switch(),
        room.clone(),
        Some(events.clone()),
    );
    if saving {
        ops
    } else {
        ops.without_store()
    }
}

pub(crate) fn shells_ops(layout: &Layout, identity: Arc<Identifier>) -> Arc<ShellsOps> {
    Arc::new(
        ShellsOps::new(layout.roots()).with_owner(Arc::new(move |caller, session| {
            caller.pid.and_then(|pid| identity.owner_pid(pid, session))
        })),
    )
}
