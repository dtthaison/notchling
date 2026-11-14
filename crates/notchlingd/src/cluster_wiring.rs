use crate::acting_ops::Room;
use crate::cluster_join::{JoinAsker, JOIN_WAIT};
use crate::cluster_node::{
    ClusterNode, ClusterSetup, Hooks, JoinAsk, LocalCalls, NetSide, Probe, Submit, TICK,
};
use crate::cluster_ops::{follow_switch, ClusterOps};
use crate::cluster_switches::{HeadlessSwitches, LateHandler, NodeCalls, OpsPanel};
use crate::cluster_versions::{own, with_tools};
use crate::inbox_ops::InboxOps;
use crate::layout::Layout;
use crate::switches::SwitchFlag;
use crate::versions_ops::VersionsOps;
use notchling_cluster_net::{load_or_make, Pins, TcpTls, PORT};
use notchling_engine::bridge::net::bridge::host_name;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::Desired;
use notchling_local_api::spawn::in_cargo_target;
use notchling_local_api::{ApiError, AuditLog, Caller, ClientKind, ErrorCode, Handler, Tier};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

pub const LOOPBACK_ENV: &str = "NOTCHLING_CLUSTER_LOOPBACK";
pub const IDENTITY_DIR: &str = "node";

pub fn loopback_ok(env: Option<&OsStr>, exe: Option<&Path>) -> bool {
    env == Some(OsStr::new("1")) && exe.is_some_and(in_cargo_target)
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
}

impl Hooks for ClusterHooks {
    fn join_request(&self, ask: JoinAsk, node: Submit) {
        self.asker.ask(ask, node);
    }

    fn desired(&self, desired: Desired) {
        self.switches.follow(desired);
    }
}

pub struct ClusterParts {
    pub node: Result<Arc<ClusterNode>, String>,
    pub ops: Arc<ClusterOps>,
    handler: Arc<LateHandler>,
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
) -> Result<Arc<ClusterNode>, String> {
    let cluster_dir = layout.cluster_dir();
    let identity = load_or_make(&cluster_dir.join(IDENTITY_DIR)).map_err(|e| e.to_string())?;
    let fingerprint = identity.fingerprint.clone();
    let exe = std::env::current_exe().ok();
    let loopback = loopback_ok(std::env::var_os(LOOPBACK_ENV).as_deref(), exe.as_deref());
    let pins = Pins::default();
    let transport = Arc::new(TcpTls::new(identity, pins.clone(), loopback));
    let probe: Probe = Box::new(move || with_tools(versions.tool_versions()));
    let setup = ClusterSetup {
        cluster_dir,
        fixture: layout.fixture,
        loopback_ok: loopback,
        port: PORT,
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
    let audit = AuditLog::new(layout.audit_path());
    ClusterNode::start(setup, net, room, links.0, links.1, audit)
}

impl ClusterParts {
    pub fn build(
        layout: &Layout,
        room: Arc<dyn Room>,
        link: Arc<SwitchFlag>,
        versions: Arc<VersionsOps>,
        inbox: Arc<InboxOps>,
    ) -> ClusterParts {
        let handler = Arc::new(LateHandler::default());
        let panel = Arc::new(OpsPanel::new(handler.clone()));
        let switches = Arc::new(HeadlessSwitches::new(
            panel,
            room.clone(),
            AuditLog::new(layout.audit_path()),
        ));
        let local: Arc<dyn LocalCalls> = Arc::new(NodeCalls {
            inner: DaemonCalls::new(handler.clone(), room.clone()),
            switches: switches.clone(),
        });
        let hooks: Arc<dyn Hooks> = Arc::new(ClusterHooks {
            asker: JoinAsker::new(inbox, JOIN_WAIT),
            switches,
        });
        let node = start_node(layout, room.clone(), (local, hooks), versions);
        if let Ok(node) = &node {
            follow_switch(&link, node);
        }
        let audit = AuditLog::new(layout.audit_path());
        let ops = Arc::new(ClusterOps::new(node.clone(), link, room, audit));
        ClusterParts { node, ops, handler }
    }

    pub fn serve(&self, handler: &Arc<dyn Handler>) {
        self.handler.serve(handler);
    }

    pub fn close(&self) {
        if let Ok(node) = &self.node {
            node.close();
        }
    }
}

#[cfg(test)]
#[path = "cluster_wiring_tests.rs"]
mod tests;
