use crate::audit::{AuditEntry, AuditLog};
use crate::hub::Hub;
use crate::lock::{lock_path_for, InstanceLock};
use crate::tier::Allowlist;
use crate::transport::{self, Listener};
use crate::wire::{ApiError, ClientKind, DaemonStatus, Tier};
use serde_json::{Map, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpSpec {
    pub tier: Tier,
    pub approval: bool,
}

impl OpSpec {
    pub const AGENT: OpSpec = OpSpec {
        tier: Tier::Agent,
        approval: false,
    };
    pub const APP: OpSpec = OpSpec {
        tier: Tier::App,
        approval: false,
    };
    pub const APPROVAL: OpSpec = OpSpec {
        tier: Tier::Agent,
        approval: true,
    };
}

#[derive(Clone, Debug)]
pub struct Caller {
    pub tier: Tier,
    pub client: ClientKind,
    pub peer: Option<PathBuf>,
    pub pid: Option<u32>,
}

pub trait Handler: Send + Sync + 'static {
    fn spec(&self, op: &str) -> Option<OpSpec>;
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError>;
    fn call_with_extras(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> (Result<Value, ApiError>, Map<String, Value>) {
        (self.call(caller, op, args), Map::new())
    }
    fn topics(&self) -> &[&str] {
        &[]
    }
    fn topic_tier(&self, _topic: &str) -> Tier {
        Tier::Agent
    }
    fn acting(&self) -> Option<bool> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_clients: usize,
    pub app_reserved: usize,
    pub hello_within: Duration,
    pub idle_within: Duration,
    pub line_within: Duration,
    pub write_within: Duration,
    pub queue: usize,
    pub tick: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_clients: 64,
            app_reserved: 4,
            hello_within: Duration::from_secs(5),
            idle_within: Duration::from_secs(60),
            line_within: Duration::from_secs(10),
            write_within: Duration::from_secs(10),
            queue: 256,
            tick: Duration::from_millis(200),
        }
    }
}

pub struct ServerConfig {
    pub socket: PathBuf,
    pub allowlist: Allowlist,
    pub audit: Option<AuditLog>,
    pub version: String,
    pub limits: Limits,
}

pub(crate) struct Shared {
    pub(crate) handler: Arc<dyn Handler>,
    pub(crate) allowlist: Allowlist,
    pub(crate) audit: Option<AuditLog>,
    pub(crate) version: String,
    pub(crate) limits: Limits,
    pub(crate) hub: Hub,
    pub(crate) started: Instant,
    pub(crate) stop: AtomicBool,
    pub(crate) my_uid: u32,
    pub(crate) open: AtomicUsize,
}

pub(crate) struct Slot {
    shared: Arc<Shared>,
    pub(crate) reserved: bool,
}

impl Slot {
    fn take(shared: &Arc<Shared>) -> Option<Slot> {
        let limits = shared.limits;
        let open = shared.open.load(Ordering::SeqCst);
        if open >= limits.max_clients {
            return None;
        }
        shared.open.fetch_add(1, Ordering::SeqCst);
        let reserved = open >= limits.max_clients.saturating_sub(limits.app_reserved);
        Some(Slot {
            shared: shared.clone(),
            reserved,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.shared.open.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Shared {
    pub(crate) fn audit(&self, op: &str, tier: Option<Tier>, peer: Option<&Path>, result: &str) {
        self.audit_with(op, tier, peer, result, None);
    }

    pub(crate) fn audit_with(
        &self,
        op: &str,
        tier: Option<Tier>,
        peer: Option<&Path>,
        result: &str,
        reason: Option<String>,
    ) {
        if let Some(log) = &self.audit {
            let entry = AuditEntry::now(op, tier, peer, result).with_reason(reason);
            let _ = log.append(&entry);
        }
    }

    pub(crate) fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub(crate) fn status(&self) -> DaemonStatus {
        DaemonStatus {
            version: self.version.clone(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            clients: self.hub.counts(),
            acting: self.handler.acting(),
        }
    }
}

pub struct Server {
    listener: Listener,
    socket: PathBuf,
    shared: Arc<Shared>,
    _lock: InstanceLock,
}

#[derive(Clone)]
pub struct ServerHandle {
    shared: Arc<Shared>,
}

impl ServerHandle {
    pub fn publish(&self, topic: &str, event: &str, data: &Value) -> usize {
        match crate::framing::encode_line(&crate::wire::event_line(event, data)) {
            Some(line) => self.shared.hub.publish(topic, &line),
            None => 0,
        }
    }

    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.hub.close_all();
    }

    pub fn status(&self) -> DaemonStatus {
        self.shared.status()
    }

    pub fn app_attached(&self) -> bool {
        self.shared.hub.app_client_attached()
    }

    pub fn app_following(&self, topic: &str) -> bool {
        self.shared.hub.app_client_following(topic)
    }

    pub fn connections(&self) -> usize {
        self.shared.hub.len()
    }
}

pub fn already_running(socket: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("another notchling daemon is serving {}", socket.display()),
    )
}

impl Server {
    pub fn bind(config: ServerConfig, handler: Arc<dyn Handler>) -> io::Result<Server> {
        let socket = config.socket.clone();
        if let Some(parent) = socket.parent().filter(|p| !p.as_os_str().is_empty()) {
            if !parent.exists() {
                notchling_platform::files::ensure_private_dir(parent).map_err(io::Error::other)?;
            }
        }
        let lock = InstanceLock::try_acquire(&lock_path_for(&socket))?
            .ok_or_else(|| already_running(&socket))?;
        match transport::is_socket(&socket) {
            Ok(true) => std::fs::remove_file(&socket)?,
            Ok(false) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a socket", socket.display()),
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = transport::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Shared {
            handler,
            allowlist: config.allowlist,
            audit: config.audit,
            version: config.version,
            limits: config.limits,
            hub: Hub::default(),
            started: Instant::now(),
            stop: AtomicBool::new(false),
            my_uid: crate::peer::current_uid(),
            open: AtomicUsize::new(0),
        });
        Ok(Server {
            listener,
            socket,
            shared,
            _lock: lock,
        })
    }

    pub fn handle(&self) -> ServerHandle {
        ServerHandle {
            shared: self.shared.clone(),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn run(self) -> io::Result<()> {
        let result = self.accept_loop();
        self.shared.hub.close_all();
        let _ = std::fs::remove_file(&self.socket);
        result
    }

    pub fn spawn(self) -> io::Result<(ServerHandle, JoinHandle<io::Result<()>>)> {
        let handle = self.handle();
        let thread = std::thread::Builder::new()
            .name("notchling-api".into())
            .spawn(move || self.run())?;
        Ok((handle, thread))
    }

    fn accept_loop(&self) -> io::Result<()> {
        while !self.shared.stopping() {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let Some(slot) = Slot::take(&self.shared) else {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        continue;
                    };
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let shared = self.shared.clone();
                    let spawned = std::thread::Builder::new()
                        .name("notchling-api-conn".into())
                        .spawn(move || crate::conn::serve(stream, shared, slot));
                    if spawned.is_err() {
                        continue;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || passing(&e) => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

fn passing(e: &io::Error) -> bool {
    if matches!(
        e.kind(),
        io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
    ) {
        return true;
    }
    #[cfg(unix)]
    if let Some(code) = e.raw_os_error() {
        return [libc::EMFILE, libc::ENFILE, libc::ENOBUFS, libc::ENOMEM].contains(&code);
    }
    false
}

#[cfg(test)]
mod accept_tests {
    use super::passing;
    use std::io;

    #[test]
    fn a_client_giving_up_or_a_short_of_descriptors_moment_keeps_the_server_up() {
        assert!(passing(&io::Error::from(io::ErrorKind::ConnectionAborted)));
        assert!(passing(&io::Error::from(io::ErrorKind::ConnectionReset)));
        #[cfg(unix)]
        {
            assert!(passing(&io::Error::from_raw_os_error(libc::EMFILE)));
            assert!(passing(&io::Error::from_raw_os_error(libc::ENFILE)));
            assert!(!passing(&io::Error::from_raw_os_error(libc::EBADF)));
            assert!(!passing(&io::Error::from_raw_os_error(libc::EINVAL)));
        }
        assert!(!passing(&io::Error::from(io::ErrorKind::PermissionDenied)));
    }
}
