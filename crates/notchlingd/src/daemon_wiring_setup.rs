use crate::acting_ops::{ActingOps, Room};
use crate::bridge_backend::APPROVAL_WAIT;
use crate::bridge_ops::BridgeSetup;
use crate::identity::Identifier;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::outbox_ops::OutboxOps;
use crate::shells_ops::{Progress, ShellsOps};
use crate::shells_watch::{candidates_in, ShellsWatch};
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

pub(crate) fn shells_ops(
    layout: &Layout,
    identity: Arc<Identifier>,
    progress: Progress,
) -> Arc<ShellsOps> {
    Arc::new(
        ShellsOps::new(layout.roots())
            .with_progress(progress)
            .with_owner(Arc::new(move |caller, session| {
                caller.pid.and_then(|pid| identity.owner_pid(pid, session))
            })),
    )
}

pub(crate) fn shells_watch(
    layout: &Layout,
    inbox: &Arc<InboxOps>,
    acting: &Arc<ActingOps>,
    sessions: &Arc<Mutex<Value>>,
) -> Arc<ShellsWatch> {
    Arc::new(ShellsWatch::new(
        Arc::new(ShellsOps::new(layout.roots())),
        inbox.clone(),
        acting.acting_switch(),
        candidates_in(sessions.clone()),
    ))
}

pub(crate) fn reads(
    wired: &crate::daemon_wiring::Wired,
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
) -> Arc<crate::reads_ops::ReadsOps> {
    Arc::new(crate::reads_ops::ReadsOps::new(
        notchling_engine::organizations::Organizations::new(&layout.data_dir)
            .with_home(&layout.home()),
        &layout.home(),
        sessions.clone(),
        wired.inbox.clone(),
        wired.providers.clone(),
        wired.accounts.store(),
        wired.limits.clone(),
        &layout.data_dir.join("daemon"),
        Arc::new(notchling_engine::transcript::util::now_secs),
    ))
}

pub(crate) fn session_asks(
    wired: &crate::daemon_wiring::Wired,
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
    reads: Arc<crate::reads_ops::ReadsOps>,
) -> Arc<crate::session_asks::SessionAsks> {
    let directory = crate::session_live::LiveDirectory::new(
        sessions.clone(),
        wired.every.clone(),
        reads,
        layout.roots(),
    );
    let acting = wired.acting.clone();
    crate::session_asks::SessionAsks::new(
        wired.consent.clone(),
        Arc::new(directory),
        crate::session_live::runner(
            wired.acting.clone(),
            wired.codex.clone(),
            wired.agy.clone(),
            wired.outbox.clone(),
        ),
        Arc::new(crate::session_children::Children::open(
            layout.data_dir.join("daemon").join("session-children.json"),
        )),
        Arc::new(move || acting.is_on()),
    )
}

pub(crate) fn job_asks(
    wired: &crate::daemon_wiring::Wired,
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
) -> Arc<crate::job_asks::JobAsks> {
    use notchling_engine::machines::registry::RegistryFiles;
    use notchling_engine::machines::roots::MachineRoots;
    use notchling_engine::organizations::Organizations;
    let registry = RegistryFiles {
        read_only: true,
        ..MachineRoots::in_data(&layout.data_dir).registry()
    };
    let orgs = Organizations::new(&layout.data_dir).with_home(&layout.home());
    let cluster = &wired.cluster;
    if let Some(done) = cluster.dispatch.asks() {
        return done;
    }
    let hands = crate::job_live::LiveHands::new(
        &cluster.dispatch,
        cluster.returns.clone(),
        cluster.shells.clone(),
        sessions.clone(),
        cluster.tails.clone(),
    );
    let on = wired.switches.cluster.clone();
    let asks = crate::job_asks::JobAsks::new(
        wired.consent.clone(),
        Arc::new(crate::job_live::LiveFleet::new(
            cluster.node.clone(),
            registry,
            orgs,
        )),
        Arc::new(hands),
        Arc::new(crate::job_book::JobBook::open(
            layout.data_dir.join("daemon").join("jobs.json"),
        )),
        Arc::new(move || on.is_on()),
    );
    cluster.dispatch.set_asks(asks.clone());
    asks
}

pub(crate) fn account_asks(
    wired: &crate::daemon_wiring::Wired,
    layout: &Layout,
) -> Arc<crate::account_asks::AccountAsks> {
    let book = crate::account_asks_live::LiveBook::new(
        wired.accounts.store(),
        wired.providers.clone(),
        wired.acting.clone(),
        notchling_engine::organizations::Organizations::new(&layout.data_dir)
            .with_home(&layout.home()),
        &layout.data_dir.join("daemon"),
        wired.switches.accounts.clone(),
        &layout.home(),
    );
    crate::account_asks::AccountAsks::new(
        wired.consent.clone(),
        Arc::new(book),
        wired.account_follow.clone(),
    )
}

pub(crate) fn cluster_asks(
    wired: &crate::daemon_wiring::Wired,
    layout: &Layout,
) -> Arc<crate::cluster_asks::ClusterAsks> {
    use notchling_engine::machines::registry::RegistryFiles;
    use notchling_engine::machines::roots::MachineRoots;
    use notchling_engine::organizations::Organizations;
    let registry = RegistryFiles {
        read_only: true,
        ..MachineRoots::in_data(&layout.data_dir).registry()
    };
    let orgs = Organizations::new(&layout.data_dir).with_home(&layout.home());
    let on = wired.switches.cluster.clone();
    crate::cluster_asks::ClusterAsks::new(
        wired.consent.clone(),
        Arc::new(crate::cluster_asks_live::LiveAdmin::new(
            wired.cluster.node.clone(),
            registry,
            orgs,
            Arc::new(notchling_engine::machines::process::ProcessRunner),
        )),
        Arc::new(move || on.is_on()),
    )
}
