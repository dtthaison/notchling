use crate::acting_ops::Room;
use crate::cluster_blobs::Blobs;
use crate::cluster_dispatch::{DispatchCalls, DispatchOps, NodeReporter};
use crate::cluster_dispatch_delegate::RealDelegate;
use crate::cluster_forward::{Forward, NoNodeForward, NodeForward};
use crate::cluster_inbox::{InboxCalls, NetLink, NodeNet, Outbox};
use crate::cluster_inbox_follow::Follow;
use crate::cluster_join::{JoinAsker, JOIN_WAIT};
use crate::cluster_logins::{LoginCalls, LoginEnv, LoginHold, CLUSTER_LOGIN_ON};
use crate::cluster_logins_probe::{has_content, LoginProbe, SystemStatus};
use crate::cluster_logins_send::{LoginDeliverer, NodeOnline, Parts, Stamps};
use crate::cluster_mirror::{FollowOps, Mirror, OnCap, TailSource, MIRROR_CAP, MIRROR_DIR};
use crate::cluster_node::{
    ClusterLog, ClusterNode, ClusterSetup, Hooks, JoinAsk, LocalCalls, NetSide, Probe, Submit, TICK,
};
use crate::cluster_ops::{follow_switch, ClusterOps};
pub use crate::cluster_publisher::Publisher;
use crate::cluster_sessions::{
    is_client, HandlerRows, NodeSource, Refresher, RemoteSessions, SessionCalls,
};
use crate::cluster_switches::{HeadlessSwitches, LateHandler, NodeCalls, OpsPanel};
use crate::cluster_tail_mask::MaskedTails;
use crate::cluster_versions::{own, with_logins, with_probed, with_tools};
use crate::cluster_workspace::{Workspace, WorkspaceCalls};
use crate::cluster_workspace_send::{CodeSender, NodePeers, ReturnOps, Sender};
use crate::grant_broker::GrantBroker;
use crate::grant_calls::{
    ClusterUseAudit, EventAudit, GrantCalls, InboxNotices, NodeAsker, NodeLink, NodeView,
};
use crate::grant_janitor::GrantJanitor;
use crate::grant_use::{GrantUse, OnDisk, UseCalls};
use crate::grant_wiring::GrantParts;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::shell_jobs::ShellJobs;
use crate::switches::SwitchFlag;
use crate::vault_caller::SystemProcesses;
use crate::vault_ops::VaultOps;
use crate::versions_ops::VersionsOps;
use notchling_cluster_net::{load_or_make, Pins, TcpTls, PORT};
use notchling_engine::bridge::net::bridge::host_name;
use notchling_engine::cluster::login_files::LoginStore;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{Desired, NodeId};
use notchling_engine::control::{Launcher, RealLauncher};
use notchling_events::kinds::security::{ClusterStep, SwitchCause};
use notchling_events::spec::Outcome;
use notchling_local_api::spawn::in_cargo_target;
use notchling_local_api::{topics, ApiError, Caller, ClientKind, ErrorCode, Handler, Tier};
use serde_json::{json, Value};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub const LOOPBACK_ENV: &str = "NOTCHLING_CLUSTER_LOOPBACK";
pub const PORT_ENV: &str = "NOTCHLING_CLUSTER_PORT";
pub const IDENTITY_DIR: &str = "node";

pub fn loopback_ok(env: Option<&OsStr>, exe: Option<&Path>) -> bool {
    env == Some(OsStr::new("1")) && exe.is_some_and(in_cargo_target)
}

pub fn cluster_port(loopback: bool, raw: Option<&OsStr>) -> u16 {
    if !loopback {
        return PORT;
    }
    raw.and_then(OsStr::to_str)
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(PORT)
}

pub fn accepts_delegation(status: &Value) -> bool {
    let me = status.get("me").and_then(Value::as_str);
    status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|m| m.get("id").and_then(Value::as_str) == me)
        .and_then(|m| m.get("acceptsDelegation").and_then(Value::as_bool))
        .unwrap_or(false)
}

pub fn node_caller(origin: &Origin) -> Caller {
    Caller {
        tier: match origin.tier {
            OriginTier::App => Tier::App,
            OriginTier::Agent | OriginTier::Node => Tier::Agent,
        },
        client: ClientKind::Node,
        peer: None,
        pid: None,
    }
}

pub fn checked(
    handler: &dyn Handler,
    room: &dyn Room,
    caller: &Caller,
    op: &str,
    args: &Value,
) -> Result<Value, ApiError> {
    let Some(spec) = handler.spec(op) else {
        return Err(ApiError::new(
            ErrorCode::UnknownOp,
            format!("no op named {op}"),
        ));
    };
    if spec.tier == Tier::App && caller.tier != Tier::App {
        return Err(ApiError::new(
            ErrorCode::Forbidden,
            "only the Notchling app may do this",
        ));
    }
    if spec.approval && !room.can_answer() {
        return Err(ApiError::new(
            ErrorCode::NeedsApproval,
            "this needs approval in the Notchling app, and the app isn't attached",
        ));
    }
    handler.call(caller, op, args)
}

pub struct DaemonCalls {
    handler: Arc<LateHandler>,
    room: Arc<dyn Room>,
}

impl DaemonCalls {
    pub fn new(handler: Arc<LateHandler>, room: Arc<dyn Room>) -> Arc<DaemonCalls> {
        Arc::new(DaemonCalls { handler, room })
    }
}

impl LocalCalls for DaemonCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let handler = self.handler.get()?;
        checked(
            handler.as_ref(),
            self.room.as_ref(),
            &node_caller(origin),
            op,
            args,
        )
    }
}

pub struct ClusterHooks {
    pub asker: JoinAsker,
    pub switches: Arc<HeadlessSwitches>,
    pub remote: Arc<RemoteSessions>,
    pub mirror: Arc<Mirror>,
    pub inbox: Arc<InboxOps>,
    pub login: Arc<LoginHold>,
}

impl Hooks for ClusterHooks {
    fn join_request(&self, ask: JoinAsk, node: Submit) {
        self.asker.ask(ask, node);
    }

    fn desired(&self, desired: Desired) {
        self.switches.follow(desired);
    }

    fn wiped(&self) {
        self.mirror.clear();
        self.remote.clear();
        self.inbox.clear_mirrors();
        self.login.wipe();
    }

    fn peer_sessions(&self, from: &NodeId) {
        self.remote.mark_dirty(from);
        self.mirror.nudge();
    }
}

#[derive(Default)]
struct LateTail(OnceLock<NodeSource>);

impl TailSource for LateTail {
    fn tail(
        &self,
        node: &NodeId,
        id: &str,
        from: u64,
        seen: Option<&str>,
    ) -> Result<Value, ApiError> {
        match self.0.get() {
            Some(source) => source.tail(node, id, from, seen),
            None => Err(ApiError::new(
                ErrorCode::Failed,
                "the cluster node isn't up",
            )),
        }
    }
}

pub fn rejoin_on_start(
    link: &SwitchFlag,
    node: &ClusterNode,
    room: &dyn Room,
    log: &ClusterLog,
) -> bool {
    let Some(home) = node.rejoin_home() else {
        return false;
    };
    if link.is_on() || room.can_answer() {
        return false;
    }
    link.set(true);
    log.switch(link.name(), true, SwitchCause::ClusterRejoin, Outcome::Ok);
    log.step(
        ClusterStep::Rejoined,
        Some(home.as_str()),
        None,
        Outcome::Ok,
    );
    let data = json!({"switch": link.name(), "on": true});
    room.publish(topics::SETTINGS, topics::SWITCH_CHANGED, &data);
    true
}

pub struct ClusterParts {
    pub node: Result<Arc<ClusterNode>, String>,
    pub ops: Arc<ClusterOps>,
    pub follow: Arc<FollowOps>,
    pub dispatch: Arc<DispatchOps>,
    pub returns: Arc<ReturnOps>,
    pub shells: Arc<ShellJobs>,
    pub remote: Arc<RemoteSessions>,
    pub login: Arc<LoginHold>,
    logins: Arc<LoginDeliverer>,
    delegate: Arc<RealDelegate>,
    mirror: Arc<Mirror>,
    janitor: Arc<GrantJanitor>,
    outbox: Arc<Outbox>,
    publisher: Arc<Publisher>,
    refresher: Option<Refresher>,
    handler: Arc<LateHandler>,
}

fn clock() -> crate::cluster_mirror::Clock {
    Arc::new(|| u64::try_from(crate::acting_ops::now_ms()).unwrap_or(0))
}

pub(crate) fn on_disk(cluster_dir: &Path) -> OnDisk {
    let state = match notchling_engine::cluster::persist::load(cluster_dir) {
        Ok(Some(state)) => state,
        Ok(None) => return OnDisk::Nothing,
        Err(_) => return OnDisk::Unreadable,
    };
    let me = notchling_cluster_net::stored_fingerprint(&cluster_dir.join(IDENTITY_DIR))
        .ok()
        .flatten()
        .and_then(|fingerprint| NodeId::from_fingerprint(&fingerprint));
    OnDisk::Cluster {
        vault: state.vault,
        me,
    }
}

fn disk_vault(dir: PathBuf) -> crate::grant_use::Disk {
    Arc::new(move || on_disk(&dir))
}

fn clock_i64() -> crate::cluster_inbox::Clock {
    Arc::new(crate::acting_ops::now_ms)
}

fn clock_ms() -> crate::grant_broker::Clock {
    Arc::new(crate::acting_ops::now_ms)
}

fn node_name() -> String {
    let name = host_name().unwrap_or_else(|| "node".to_string());
    name.strip_suffix(".local").unwrap_or(&name).to_string()
}

fn start_node(
    layout: &Layout,
    room: Arc<dyn Room>,
    links: (Arc<dyn LocalCalls>, Arc<dyn Hooks>),
    versions: Arc<VersionsOps>,
    (log, login, probed): (ClusterLog, Arc<LoginHold>, Arc<LoginProbe>),
) -> Result<Arc<ClusterNode>, String> {
    let cluster_dir = layout.cluster_dir();
    let identity = load_or_make(&cluster_dir.join(IDENTITY_DIR)).map_err(|e| e.to_string())?;
    let fingerprint = identity.fingerprint.clone();
    let exe = std::env::current_exe().ok();
    let loopback = loopback_ok(std::env::var_os(LOOPBACK_ENV).as_deref(), exe.as_deref());
    let pins = Pins::default();
    let transport = Arc::new(TcpTls::new(identity, pins.clone(), loopback));
    let probe: Probe = Box::new(move || {
        let found = with_logins(with_tools(versions.tool_versions()), login.standing());
        with_probed(found, probed.logins())
    });
    let setup = ClusterSetup {
        cluster_dir,
        fixture: layout.fixture,
        loopback_ok: loopback,
        port: cluster_port(loopback, std::env::var_os(PORT_ENV).as_deref()),
        versions: own(),
        client: cfg!(target_os = "macos"),
        name: node_name(),
        tick: TICK,
        probe: Some(probe),
    };
    let net = NetSide {
        transport,
        pins,
        fingerprint,
    };
    ClusterNode::start(setup, net, room, links.0, links.1, log)
}

impl ClusterParts {
    pub fn build(
        layout: &Layout,
        room: Arc<dyn Room>,
        link: Arc<SwitchFlag>,
        versions: Arc<VersionsOps>,
        (inbox, vault): (Arc<InboxOps>, Arc<VaultOps>),
        events: Events,
        sessions: Arc<Mutex<Value>>,
    ) -> ClusterParts {
        let log = ClusterLog::new(events);
        let handler = Arc::new(LateHandler::default());
        let panel = Arc::new(OpsPanel::new(handler.clone()));
        let switches = Arc::new(HeadlessSwitches::new(panel, room.clone(), log.clone()));
        let mirror_dir = layout.cluster_dir().join(MIRROR_DIR);
        let holder: Arc<OnceLock<Arc<ClusterNode>>> = Arc::new(OnceLock::new());
        let gate = holder.clone();
        let remote = Arc::new(RemoteSessions::new(mirror_dir.clone()).with_gate(Arc::new(
            move || gate.get().is_some_and(|node| is_client(&node.status())),
        )));
        let publisher = Arc::new(Publisher::new(room.clone(), remote.clone(), sessions));
        let tail = Arc::new(LateTail::default());
        let capped: OnCap = {
            let log = log.clone();
            Arc::new(move |node: &NodeId| {
                log.step(
                    ClusterStep::Refused,
                    Some(node.as_str()),
                    None,
                    Outcome::Failed,
                )
            })
        };
        let mirror = Mirror::with_cap(
            mirror_dir,
            remote.clone(),
            tail.clone(),
            clock(),
            (MIRROR_CAP, capped),
        );
        let reporter = Arc::new(NodeReporter::default());
        let grants = GrantParts::new(&layout.cluster_dir());
        let view = Arc::new(NodeView::new(holder.clone()));
        let login = LoginHold::new(
            (
                Arc::new(LoginStore::new(&layout.cluster_dir())),
                grants.masks.clone(),
                view.clone(),
            ),
            (&layout.cluster_dir(), CLUSTER_LOGIN_ON),
            clock_ms(),
        );
        let shells =
            ShellJobs::new(layout.data_dir.clone(), reporter.clone()).with_grants(grants.shell());
        let delegate = Arc::new(RealDelegate::new(layout));
        let asks = holder.clone();
        let takes: crate::cluster_dispatch::Takes = Arc::new(move || {
            asks.get()
                .is_some_and(|node| accepts_delegation(&node.status()))
        });
        let closing = shells.clone();
        let sessions_calls: Arc<dyn LocalCalls> = Arc::new(MaskedTails::new(
            Arc::new(SessionCalls::new(
                DaemonCalls::new(handler.clone(), room.clone()),
                Arc::new(HandlerRows::new(handler.clone())),
                room.clone(),
            )),
            Arc::new(HandlerRows::new(handler.clone())),
            (grants.masks.clone(), view.clone()),
            Arc::new(move |job: &str| closing.log_closed(job)),
        ));
        let launcher: Arc<dyn Launcher> =
            Arc::new(RealLauncher::with_claude(PathBuf::from("claude")));
        let blobs = Blobs::new(layout.cluster_dir());
        let workspace = Workspace::new(layout.cluster_dir(), launcher.clone(), blobs);
        let sender = Sender::new(
            layout.cluster_dir(),
            launcher,
            Arc::new(NodePeers::new(holder.clone())),
        );
        let workspace_calls: Arc<dyn LocalCalls> =
            Arc::new(WorkspaceCalls::new(sessions_calls, workspace));
        let uses = Arc::new(
            GrantUse::new(
                grants.store.clone(),
                grants.parts(),
                view.clone(),
                (
                    Arc::new(NodeAsker::new(holder.clone())),
                    Arc::new(ClusterUseAudit::new(log.clone())),
                ),
                (Arc::new(SystemProcesses), clock_ms()),
            )
            .with_disk(disk_vault(layout.cluster_dir())),
        );
        vault.set_worker(uses.clone());
        let janitor = GrantJanitor::new(
            grants.store.clone(),
            grants.parts(),
            view.clone(),
            clock_ms(),
        );
        let workspace_calls: Arc<dyn LocalCalls> = Arc::new(UseCalls::new(workspace_calls, uses));
        let logins = LoginDeliverer::new(
            Parts {
                vault: vault.clone(),
                entries: vault.clone(),
                link: Arc::new(NodeLink::new(holder.clone())),
                audit: Arc::new(EventAudit::new(vault.clone(), log.clone())),
                view: view.clone(),
                online: Arc::new(NodeOnline::new(holder.clone())),
                notes: inbox.clone(),
            },
            Stamps::load(&layout.cluster_dir()),
            clock_ms(),
        );
        let broker = Arc::new(GrantBroker::new(
            (
                vault.clone(),
                Arc::new(NodeLink::new(holder.clone())),
                Arc::new(EventAudit::new(vault, log.clone())),
            ),
            Arc::new(InboxNotices::new(inbox.clone())),
            view,
            clock_ms(),
        ));
        let granting: Arc<dyn LocalCalls> = Arc::new(GrantCalls::new(workspace_calls, broker));
        let granting: Arc<dyn LocalCalls> = Arc::new(LoginCalls::new(granting, login.clone()));
        let net = Arc::new(NodeNet::new(holder.clone()));
        let granting: Arc<dyn LocalCalls> =
            Arc::new(InboxCalls::new(granting, net.clone(), inbox.clone()));
        let outbox = Outbox::new(net.clone(), inbox.clone(), clock_i64());
        let local: Arc<dyn LocalCalls> = Arc::new(NodeCalls {
            inner: Arc::new(DispatchCalls::new(
                granting,
                shells.clone(),
                delegate.clone(),
                takes,
            )),
            switches: switches.clone(),
        });
        let hooks: Arc<dyn Hooks> = Arc::new(ClusterHooks {
            asker: JoinAsker::new(inbox.clone(), JOIN_WAIT),
            switches,
            remote: remote.clone(),
            mirror: mirror.clone(),
            inbox: inbox.clone(),
            login: login.clone(),
        });
        let probed = LoginProbe::new(
            (
                versions.clone(),
                Arc::new(SystemStatus),
                Arc::new(has_content),
            ),
            (inbox.clone(), Arc::new(crate::acting_ops::now_ms)),
            (node_name(), std::env::consts::OS.to_string()),
        );
        let node = start_node(
            layout,
            room.clone(),
            (local, hooks),
            versions,
            (log.clone(), login.clone(), probed),
        );
        let audit = log.clone();
        let mut refresher = None;
        if let Ok(node) = &node {
            reporter.attach(node.clone());
            follow_switch(&link, node);
            rejoin_on_start(&link, node, room.as_ref(), &audit);
            let _ = tail.0.set(NodeSource::new(node.clone()));
            let _ = holder.set(node.clone());
            mirror.spawn();
            janitor.spawn();
            if CLUSTER_LOGIN_ON {
                logins.spawn();
            }
            let link = Arc::new(NetLink::new(net));
            inbox.set_link(link.clone());
            inbox.set_tap(outbox.tap());
            outbox.follow(Follow::new(inbox.clone(), link));
            outbox.spawn();
            let shown = publisher.clone();
            refresher = Some(Refresher::spawn(
                node.clone(),
                remote.clone(),
                Arc::new(move || shown.publish()),
            ));
        }
        let ops = Arc::new(ClusterOps::new(node.clone(), link, room.clone(), audit));
        let code: Arc<dyn CodeSender> = sender.clone();
        let dispatch = DispatchOps::with_code(node.clone(), shells.clone(), Some(code));
        let returns = ReturnOps::new(sender);
        ClusterParts {
            node,
            ops,
            follow: Arc::new(FollowOps::new(mirror.clone())),
            dispatch,
            returns,
            shells,
            delegate,
            remote,
            login,
            logins,
            mirror,
            janitor,
            outbox,
            publisher,
            refresher,
            handler,
        }
    }

    pub fn forward(&self) -> Option<Arc<dyn Forward>> {
        Some(forwarder(&self.node))
    }

    pub fn local_changed(&self) {
        self.publisher.publish();
        if let Ok(node) = &self.node {
            node.tell(topics::SESSIONS, topics::SESSIONS_CHANGED, json!({}));
        }
    }

    pub fn serve(&self, handler: &Arc<dyn Handler>) {
        self.handler.serve(handler);
    }

    pub fn close(&self) {
        self.delegate.stop();
        self.shells.close();
        self.mirror.close();
        self.janitor.close();
        self.logins.close();
        self.outbox.close();
        if let Some(refresher) = &self.refresher {
            refresher.close();
        }
        if let Ok(node) = &self.node {
            node.close();
        }
    }
}

pub fn forwarder(node: &Result<Arc<ClusterNode>, String>) -> Arc<dyn Forward> {
    match node {
        Ok(node) => Arc::new(NodeForward::new(node.clone())),
        Err(_) => Arc::new(NoNodeForward),
    }
}

#[cfg(test)]
#[path = "cluster_wiring_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_rejoin_tests.rs"]
mod rejoin_tests;

#[cfg(test)]
#[path = "cluster_wiring_disk_tests.rs"]
mod disk_tests;
