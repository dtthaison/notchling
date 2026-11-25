use crate::client::HostClient;
use crate::host::{HostEvent, SessionHost};
use crate::wire::{self, int, Frame, FrameReader, SessionId};
use crate::HostError;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub const PROTOCOL_VERSION: i64 = 1;

pub type Authorize = Arc<dyn Fn(u32) -> bool + Send + Sync>;

pub fn same_user() -> Authorize {
    // SAFETY: getuid has no preconditions.
    let me = unsafe { libc::getuid() };
    Arc::new(move |uid| uid == me)
}

pub struct HostServer {
    host: Arc<SessionHost>,
    socket_path: PathBuf,
    authorize: Authorize,
    running: Arc<AtomicBool>,
    connections: Arc<Mutex<HashMap<u64, Arc<Connection>>>>,
    next: AtomicU64,
    this: Weak<HostServer>,
}

impl HostServer {
    pub fn new(
        host: Arc<SessionHost>,
        socket_path: &Path,
        authorize: Authorize,
    ) -> Arc<HostServer> {
        let server = Arc::new_cyclic(|this: &Weak<HostServer>| HostServer {
            host: host.clone(),
            socket_path: socket_path.to_path_buf(),
            authorize,
            running: Arc::new(AtomicBool::new(false)),
            connections: Arc::default(),
            next: AtomicU64::new(1),
            this: this.clone(),
        });
        let connections = Arc::downgrade(&server.connections);
        host.set_on_event(move |event| {
            if let Some(connections) = connections.upgrade() {
                broadcast(&connections, event);
            }
        });
        server
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn start(&self) -> Result<(), HostError> {
        let path = &self.socket_path;
        let limit = if cfg!(target_os = "macos") { 104 } else { 108 };
        if path.as_os_str().len() + 1 > limit {
            return Err(HostError::PathTooLong);
        }
        let dir = path.parent().unwrap_or(Path::new("/"));
        std::fs::create_dir_all(dir).map_err(|e| HostError::socket(&e))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| HostError::socket(&e))?;

        if path.exists() || path.symlink_metadata().is_ok() {
            if let Ok(existing) = HostClient::connect(path, Duration::from_millis(500)) {
                existing.close();
                return Err(HostError::AlreadyRunning);
            }
            let _ = std::fs::remove_file(path);
        }
        let listener = UnixListener::bind(path).map_err(|e| HostError::socket(&e))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| HostError::socket(&e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| HostError::socket(&e))?;
        self.running.store(true, Ordering::SeqCst);
        let this = self.this.clone();
        std::thread::spawn(move || accept_loop(&this, &listener));
        Ok(())
    }

    pub fn stop(&self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        let open: Vec<Arc<Connection>> = {
            let mut all = self.connections.lock().unwrap_or_else(|p| p.into_inner());
            all.drain().map(|(_, c)| c).collect()
        };
        for connection in open {
            connection.close();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }

    fn accepted(&self, stream: UnixStream) {
        let Some(uid) = peer_uid(&stream) else {
            return;
        };
        if !(self.authorize)(uid) {
            eprintln!("hostd: refused a connection from uid {uid}");
            return;
        }
        let _ = stream.set_nonblocking(false);
        let Ok(writer) = stream.try_clone() else {
            return;
        };
        let key = self.next.fetch_add(1, Ordering::SeqCst);
        let connection = Arc::new(Connection {
            host: self.host.clone(),
            writer: Mutex::new(writer),
            listeners: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
            on_close: Mutex::new(None),
        });
        let connections = Arc::downgrade(&self.connections);
        *connection
            .on_close
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Box::new(move || {
            if let Some(all) = connections.upgrade() {
                all.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
            }
        }));
        self.connections
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, connection.clone());
        std::thread::spawn(move || connection.read_loop(stream));
    }
}

fn accept_loop(server: &Weak<HostServer>, listener: &UnixListener) {
    loop {
        let Some(server) = server.upgrade() else {
            return;
        };
        if !server.running.load(Ordering::SeqCst) {
            return;
        }
        let mut pfd = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut pfd, 1, 200) };
        if ready <= 0 {
            continue;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if server.running.load(Ordering::SeqCst) {
                    set_no_sigpipe(&stream);
                    server.accepted(stream);
                }
            }
            Err(_) => continue,
        }
    }
}

pub(crate) fn set_no_sigpipe(stream: &UnixStream) {
    #[cfg(target_os = "macos")]
    {
        let on: libc::c_int = 1;
        // SAFETY: a valid socket and an int option.
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&on as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = stream;
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: valid out pointers on a connected Unix socket.
    let ok = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0;
    ok.then_some(uid)
}

#[cfg(not(target_os = "macos"))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    // SAFETY: an all-zero ucred is a valid out buffer.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred is valid for len bytes.
    let ok = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } == 0;
    ok.then_some(cred.uid)
}

fn broadcast(connections: &Mutex<HashMap<u64, Arc<Connection>>>, event: HostEvent) {
    let all: Vec<Arc<Connection>> = connections
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .cloned()
        .collect();
    let message = match event {
        HostEvent::Exited { id, status } => {
            json!({"event": "exit", "sessionId": id.to_string(), "status": status})
        }
        HostEvent::Spawned { id } => json!({"event": "spawned", "sessionId": id.to_string()}),
    };
    let Value::Object(message) = message else {
        return;
    };
    let frame = wire::json(message);
    for connection in all {
        connection.send(&frame);
    }
}

type OnClose = Box<dyn FnOnce() + Send>;

struct Connection {
    host: Arc<SessionHost>,
    writer: Mutex<UnixStream>,
    listeners: Mutex<HashMap<SessionId, u64>>,
    closed: AtomicBool,
    on_close: Mutex<Option<OnClose>>,
}

impl Connection {
    fn send(&self, bytes: &[u8]) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let mut writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writer.write_all(bytes);
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let attached: Vec<(SessionId, u64)> = self
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .collect();
        for (session, token) in attached {
            if let Some(handle) = self.host.session(session) {
                handle.remove_listener(token);
            }
        }
        {
            let writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
        let on_close = self
            .on_close
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(on_close) = on_close {
            on_close();
        }
    }

    fn read_loop(self: Arc<Self>, mut stream: UnixStream) {
        let mut reader = FrameReader::new();
        let mut buffer = vec![0u8; 65536];
        loop {
            let count = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for frame in reader.feed(&buffer[..count]) {
                self.handle(frame);
            }
            if reader.failure().is_some() {
                break;
            }
        }
        self.close();
    }

    fn handle(self: &Arc<Self>, frame: Frame) {
        match frame {
            Frame::Data { session, bytes } => {
                if let Some(handle) = self.host.session(session) {
                    handle.write(&bytes);
                }
            }
            Frame::Json(message) => {
                let (Some(id), Some(op)) = (
                    int(message.get("id")),
                    message.get("op").and_then(Value::as_str),
                ) else {
                    return;
                };
                let reply = match crate::ops::perform(self, op, &message) {
                    Ok(data) => json!({"id": id, "ok": true, "data": Value::Object(data)}),
                    Err(error) => json!({"id": id, "ok": false, "error": error.to_string()}),
                };
                if let Value::Object(reply) = reply {
                    self.send(&wire::json(reply));
                }
            }
        }
    }
}

pub(crate) trait Attachments {
    fn host(&self) -> &Arc<SessionHost>;
    fn attach(&self, session: SessionId) -> Result<(), HostError>;
    fn detach(&self, session: SessionId);
}

impl Attachments for Arc<Connection> {
    fn host(&self) -> &Arc<SessionHost> {
        &self.host
    }

    fn attach(&self, session: SessionId) -> Result<(), HostError> {
        let Some(handle) = self.host.session(session) else {
            return Err(HostError::Refused("No such session.".to_string()));
        };
        let existing = self
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&session);
        if let Some(existing) = existing {
            handle.remove_listener(existing);
        }
        let weak = Arc::downgrade(self);
        let token = handle.add_listener(Arc::new(move |bytes: &[u8]| {
            if let Some(connection) = weak.upgrade() {
                connection.send(&wire::encode(&Frame::Data {
                    session,
                    bytes: bytes.to_vec(),
                }));
            }
        }));
        self.listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(session, token);
        Ok(())
    }

    fn detach(&self, session: SessionId) {
        let token = self
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&session);
        if let (Some(token), Some(handle)) = (token, self.host.session(session)) {
            handle.remove_listener(token);
        }
    }
}

pub(crate) fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
