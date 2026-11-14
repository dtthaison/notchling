use crate::acting_ops::Room;
use crate::cluster_driver::Driver;
use notchling_cluster_net::{Conn, LinkEvents, Pins, Transport};
use notchling_engine::cluster::core::{LocalCmd, CALL_MS};
use notchling_engine::cluster::wire::{Origin, WireError};
use notchling_engine::cluster::{Desired, Input, NodeId, Versions};
use notchling_local_api::{ApiError, AuditEntry, AuditLog, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const TICK: Duration = Duration::from_secs(1);
pub const CLOSE_WAIT: Duration = Duration::from_secs(3);
pub const LOCAL_WAIT: Duration = Duration::from_millis(CALL_MS + 2_000);

pub trait LocalCalls: Send + Sync {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinAsk {
    pub item: String,
    pub node: NodeId,
    pub name: String,
    pub os: String,
    pub arch: String,
    pub addr: Option<String>,
    pub can_be_master: bool,
    pub headless: bool,
}

pub trait Hooks: Send + Sync {
    fn join_request(&self, _ask: JoinAsk, _node: Submit) {}
    fn desired(&self, _desired: Desired) {}
    fn wiped(&self) {}
}

pub struct NoHooks;

impl Hooks for NoHooks {}

pub type Probe = Box<dyn FnMut() -> Versions + Send>;

pub struct ClusterSetup {
    pub cluster_dir: PathBuf,
    pub fixture: bool,
    pub loopback_ok: bool,
    pub port: u16,
    pub versions: Versions,
    pub client: bool,
    pub name: String,
    pub tick: Duration,
    pub probe: Option<Probe>,
}

pub struct NetSide {
    pub transport: Arc<dyn Transport>,
    pub pins: Pins,
    pub fingerprint: String,
}

pub(crate) type Answer = Sender<Result<Value, WireError>>;

pub(crate) struct Parts {
    pub room: Arc<dyn Room>,
    pub local: Arc<dyn LocalCalls>,
    pub hooks: Arc<dyn Hooks>,
    pub audit: AuditLog,
}

pub(crate) enum In {
    Opened(Arc<dyn Conn>),
    Line(String, Vec<u8>),
    Closed(String, u64),
    DialDone(NodeId),
    Local(LocalCmd, Option<Answer>),
    Create(String, Answer),
    Token(Answer),
    Join(String, bool, Answer),
    Core(Input),
    Link(bool),
    Stop,
}

#[derive(Clone)]
pub struct Submit {
    tx: Sender<In>,
}

impl Submit {
    pub(crate) fn new(tx: Sender<In>) -> Submit {
        Submit { tx }
    }

    pub fn send(&self, cmd: LocalCmd) {
        let _ = self.tx.send(In::Local(cmd, None));
    }
}

pub(crate) struct Events {
    pub tx: Sender<In>,
}

impl LinkEvents for Events {
    fn opened(&self, conn: Arc<dyn Conn>) {
        let _ = self.tx.send(In::Opened(conn));
    }

    fn line(&self, peer_fp: &str, line: Vec<u8>) {
        let _ = self.tx.send(In::Line(peer_fp.to_string(), line));
    }

    fn closed(&self, peer_fp: &str, conn_id: u64) {
        let _ = self.tx.send(In::Closed(peer_fp.to_string(), conn_id));
    }
}

#[derive(Default)]
pub(crate) struct Shared {
    pub status: Mutex<Value>,
    pub rejoin: Mutex<Option<NodeId>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct ClusterNode {
    me: NodeId,
    tx: Sender<In>,
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub fn api_error(e: WireError) -> ApiError {
    let code = match e.code.as_str() {
        "forbidden" | "stale_term" => ErrorCode::Forbidden,
        "bad_args" => ErrorCode::BadRequest,
        _ => ErrorCode::Failed,
    };
    ApiError::new(code, e.message)
}

pub fn wire_error(e: ApiError) -> WireError {
    WireError::new(e.code.as_str(), e.message)
}

pub(crate) fn audit(
    log: &AuditLog,
    op: &str,
    peer: Option<&str>,
    result: &str,
    reason: Option<String>,
) {
    let mut entry = AuditEntry::now(op, None, None, result);
    entry.peer = peer.map(str::to_string);
    entry.reason = reason;
    let _ = log.append(&entry);
}

impl ClusterNode {
    pub fn start(
        setup: ClusterSetup,
        net: NetSide,
        room: Arc<dyn Room>,
        local: Arc<dyn LocalCalls>,
        hooks: Arc<dyn Hooks>,
        audit: AuditLog,
    ) -> Result<Arc<ClusterNode>, String> {
        let me = NodeId::from_fingerprint(&net.fingerprint)
            .ok_or_else(|| "the node certificate has no usable fingerprint".to_string())?;
        let (tx, rx) = channel();
        let shared = Arc::new(Shared::default());
        let mut driver = Driver::new(
            setup,
            net,
            me.clone(),
            tx.clone(),
            shared.clone(),
            Parts {
                room,
                local,
                hooks,
                audit,
            },
        );
        let thread = std::thread::Builder::new()
            .name("cluster-node".into())
            .spawn(move || driver.run(rx))
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(ClusterNode {
            me,
            tx,
            shared,
            thread: Mutex::new(Some(thread)),
        }))
    }

    pub fn me(&self) -> &NodeId {
        &self.me
    }

    pub fn status(&self) -> Value {
        let status = lock(&self.shared.status).clone();
        if status.is_null() {
            return json!({"cluster": Value::Null, "me": self.me, "members": []});
        }
        status
    }

    pub fn submit(&self) -> Submit {
        Submit::new(self.tx.clone())
    }

    pub fn set_link(&self, on: bool) {
        let _ = self.tx.send(In::Link(on));
    }

    fn wait(&self, make: impl FnOnce(Answer) -> In) -> Result<Value, ApiError> {
        let (answer, rx) = channel();
        self.tx
            .send(make(answer))
            .map_err(|_| ApiError::internal("the cluster node has stopped"))?;
        match rx.recv_timeout(LOCAL_WAIT) {
            Ok(result) => result.map_err(api_error),
            Err(_) => Err(ApiError::new(
                ErrorCode::Failed,
                "the cluster node didn't answer",
            )),
        }
    }

    pub fn local(&self, cmd: LocalCmd) -> Result<Value, ApiError> {
        self.wait(|answer| In::Local(cmd, Some(answer)))
    }

    pub fn create(&self, name: &str) -> Result<Value, ApiError> {
        self.wait(|answer| In::Create(name.to_string(), answer))
    }

    pub fn join_token(&self) -> Result<Value, ApiError> {
        self.wait(In::Token)
    }

    pub fn join(&self, token: &str, headless: bool) -> Result<Value, ApiError> {
        self.wait(|answer| In::Join(token.to_string(), headless, answer))
    }

    pub fn rejoin_home(&self) -> Option<NodeId> {
        lock(&self.shared.rejoin).clone()
    }

    pub fn close(&self) {
        let _ = self.tx.send(In::Stop);
        let Some(thread) = lock(&self.thread).take() else {
            return;
        };
        let deadline = std::time::Instant::now() + CLOSE_WAIT;
        while !thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
#[path = "cluster_node_tests.rs"]
mod tests;
