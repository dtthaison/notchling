//! The daemon's end: a listener with a thread per connection.
//!
//! [`Server::bind`] takes the single-instance lock beside the socket, clears a stale socket file
//! left by a crash, binds 0600 and starts nothing yet; [`Server::run`] accepts until
//! [`ServerHandle::shutdown`], then closes every connection and removes the socket.
//!
//! The server answers `hello`, `daemon.status` and `events.subscribe` itself and checks every other
//! op against its [`OpSpec`] before the [`Handler`] sees it: unknown → `unknown_op`; an app-tier op
//! from the agent tier → `forbidden`; an op that needs approval while no app-tier client is attached
//! → `needs_approval`. Refusals go to the audit log.

use crate::audit::{AuditEntry, AuditLog};
use crate::hub::Hub;
use crate::lock::{lock_path_for, InstanceLock};
use crate::tier::Allowlist;
use crate::transport::{self, Listener};
use crate::wire::{ApiError, ClientKind, DaemonStatus, Tier};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What an op needs before the handler runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpSpec {
    pub tier: Tier,
    /// The user must approve it in the app, so it fails closed while no app is attached.
    pub approval: bool,
}

impl OpSpec {
    /// Any client may call it.
    pub const AGENT: OpSpec = OpSpec {
        tier: Tier::Agent,
        approval: false,
    };
    /// Only the app may call it.
    pub const APP: OpSpec = OpSpec {
        tier: Tier::App,
        approval: false,
    };
    /// Any client may ask; the app must approve.
    pub const APPROVAL: OpSpec = OpSpec {
        tier: Tier::Agent,
        approval: true,
    };
}

/// Who is calling, as the server established it.
#[derive(Clone, Debug)]
pub struct Caller {
    pub tier: Tier,
    pub client: ClientKind,
    pub peer: Option<PathBuf>,
    /// The process that sent this request, from the socket's peer credentials (never from the
    /// request): on macOS the audit token of the last sender, on Linux `SO_PEERCRED`'s pid.
    /// `None` when the kernel wouldn't say.
    pub pid: Option<u32>,
}

/// The daemon's ops. Called on the connection's own thread, so a slow op holds up only its caller.
pub trait Handler: Send + Sync + 'static {
    /// The op's needs; `None` for an op that doesn't exist.
    fn spec(&self, op: &str) -> Option<OpSpec>;
    /// Runs an op the server has already checked.
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError>;
    /// The topics `events.subscribe` accepts.
    fn topics(&self) -> &[&str] {
        &[]
    }
    /// The tier a client needs to follow `topic`; the agent tier unless the handler says so.
    fn topic_tier(&self, _topic: &str) -> Tier {
        Tier::Agent
    }
    /// The acting switch, for `daemon.status`; `None` when the handler has none.
    fn acting(&self) -> Option<bool> {
        None
    }
}

/// How long a connection may take, and how many there may be.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Connections past this are closed on accept. Counted at accept, so a burst can't pass it.
    pub max_clients: usize,
    /// The last this many of `max_clients` are kept for the app tier: a connection that takes one
    /// is closed unless its peer is the app, so agents filling the rest can't lock the app out.
    pub app_reserved: usize,
    /// `hello` must arrive this soon after connecting.
    pub hello_within: Duration,
    /// A client that follows no topic is closed after this long without a request.
    pub idle_within: Duration,
    /// A line, once begun, must end this soon.
    pub line_within: Duration,
    /// A write to a client that doesn't read gives up after this.
    pub write_within: Duration,
    /// Lines waiting for one client; an event that finds it full closes the client.
    pub queue: usize,
    /// How often a waiting read wakes to check deadlines and shutdown.
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

/// What a server is built from.
pub struct ServerConfig {
    pub socket: PathBuf,
    pub allowlist: Allowlist,
    pub audit: Option<AuditLog>,
    /// The daemon's version, for `hello` and `daemon.status`.
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
    /// Connections accepted and not yet ended, hello said or not: what `max_clients` counts.
    pub(crate) open: AtomicUsize,
}

/// One of `max_clients`, taken at accept and given back when the connection's thread ends (or never
/// starts), however it ends.
pub(crate) struct Slot {
    shared: Arc<Shared>,
    /// Taken from the last `app_reserved`: only the app may keep it.
    pub(crate) reserved: bool,
}

impl Slot {
    /// A slot, or `None` when every one is taken. Only the accept loop takes slots (connections
    /// only give them back), so reading the count and then adding to it can't overshoot.
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
        if let Some(log) = &self.audit {
            // The log is best effort: a full disk must not stop the refusal itself.
            let _ = log.append(&AuditEntry::now(op, tier, peer, result));
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

/// A bound server, not yet accepting.
pub struct Server {
    listener: Listener,
    socket: PathBuf,
    shared: Arc<Shared>,
    _lock: InstanceLock,
}

/// Talks to a running server from other threads.
#[derive(Clone)]
pub struct ServerHandle {
    shared: Arc<Shared>,
}

impl ServerHandle {
    /// Sends an event to every client following `topic`; returns how many got it. Never waits on a
    /// client: one whose queue is full is closed.
    pub fn publish(&self, topic: &str, event: &str, data: &Value) -> usize {
        match crate::framing::encode_line(&crate::wire::event_line(event, data)) {
            Some(line) => self.shared.hub.publish(topic, &line),
            None => 0,
        }
    }

    /// Stops accepting and closes every connection; [`Server::run`] then returns.
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.hub.close_all();
    }

    pub fn status(&self) -> DaemonStatus {
        self.shared.status()
    }

    /// Whether the app is attached: a client with the app tier that said hello as the app, someone
    /// who can answer a prompt (the app's binary run as a hook or a command line isn't).
    pub fn app_attached(&self) -> bool {
        self.shared.hub.app_client_attached()
    }

    /// Open connections, hello said or not.
    pub fn connections(&self) -> usize {
        self.shared.hub.len()
    }
}

/// An error for "another daemon already serves this socket".
pub fn already_running(socket: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("another notchling daemon is serving {}", socket.display()),
    )
}

impl Server {
    /// Takes the lock beside the socket, removes a stale socket file and binds. Fails with
    /// `AddrInUse` while another daemon holds the lock, and refuses to remove a path that isn't a
    /// socket. A missing socket folder is made 0700; an existing one is left as it is.
    pub fn bind(config: ServerConfig, handler: Arc<dyn Handler>) -> io::Result<Server> {
        let socket = config.socket.clone();
        if let Some(parent) = socket.parent().filter(|p| !p.as_os_str().is_empty()) {
            if !parent.exists() {
                notchling_platform::files::ensure_private_dir(parent).map_err(io::Error::other)?;
            }
        }
        let lock = InstanceLock::try_acquire(&lock_path_for(&socket))?
            .ok_or_else(|| already_running(&socket))?;
        // The lock is ours, so a socket file here was left by a daemon that is gone.
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

    /// Accepts until shutdown, then removes the socket. The lock goes when this returns.
    pub fn run(self) -> io::Result<()> {
        let result = self.accept_loop();
        self.shared.hub.close_all();
        let _ = std::fs::remove_file(&self.socket);
        result
    }

    /// Runs on its own thread.
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
                    // Counted here, not when the connection's thread gets round to it, so a burst
                    // of connects can't pass the cap.
                    let Some(slot) = Slot::take(&self.shared) else {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        continue;
                    };
                    // A socket accepted from a non-blocking listener may inherit the flag (BSD).
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let shared = self.shared.clone();
                    let spawned = std::thread::Builder::new()
                        .name("notchling-api-conn".into())
                        .spawn(move || crate::conn::serve(stream, shared, slot));
                    if spawned.is_err() {
                        // Out of threads: this client is dropped, the rest keep going.
                        continue;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
