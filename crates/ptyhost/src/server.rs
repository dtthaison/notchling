use crate::client::HostClient;
use crate::host::{HostEvent, SessionHost};
use crate::wire::{self, int, Frame, FrameReader, SessionId};
use crate::HostError;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

pub const PROTOCOL_VERSION: i64 = 1;

pub type Authorize = Arc<dyn Fn(u32) -> bool + Send + Sync>;

pub fn same_user() -> Authorize {
    // SAFETY: getuid has no preconditions.
    let me = unsafe { libc::getuid() };
    Arc::new(move |uid| uid == me)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Default)]
struct Connections {
    running: bool,
    open: HashMap<u64, Arc<Connection>>,
}

struct RunningListener {
    listener: Arc<UnixListener>,
    wake: UnixStream,
    thread: JoinHandle<()>,
}

pub struct HostServer {
    host: Arc<SessionHost>,
    socket_path: PathBuf,
    authorize: Authorize,
    connections: Arc<Mutex<Connections>>,
    listener: Mutex<Option<RunningListener>>,
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
            connections: Arc::default(),
            listener: Mutex::new(None),
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
        let mut slot = lock(&self.listener);
        if slot.is_some() {
            return Err(HostError::AlreadyRunning);
        }
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
        let (wake_read, wake_write) = UnixStream::pair().map_err(|e| HostError::socket(&e))?;
        wake_read
            .set_nonblocking(true)
            .map_err(|e| HostError::socket(&e))?;
        wake_write
            .set_nonblocking(true)
            .map_err(|e| HostError::socket(&e))?;
        set_no_sigpipe(&wake_write);
        let listener = Arc::new(listener);
        lock(&self.connections).running = true;
        let this = self.this.clone();
        let for_accept = listener.clone();
        let thread = std::thread::spawn(move || accept_loop(&this, &for_accept, &wake_read));
        *slot = Some(RunningListener {
            listener,
            wake: wake_write,
            thread,
        });
        Ok(())
    }

    pub fn stop(&self) {
        let (running_listener, open) = {
            let mut slot = lock(&self.listener);
            let taken = slot.take();
            let (was_running, open) = {
                let mut all = lock(&self.connections);
                let was = all.running;
                all.running = false;
                let open: Vec<Arc<Connection>> = all.open.drain().map(|(_, c)| c).collect();
                (was, open)
            };
            if !was_running && taken.is_none() {
                return;
            }
            let _ = std::fs::remove_file(&self.socket_path);
            (taken, open)
        };
        for connection in open {
            connection.close();
        }
        if let Some(mut running) = running_listener {
            drain_listener(&running.listener);
            let _ = running.wake.write_all(&[1]);
            let _ = running.wake.shutdown(std::net::Shutdown::Both);
            drop(running.wake);
            drop(running.listener);
            if running.thread.thread().id() != std::thread::current().id() {
                let _ = running.thread.join();
            }
        }
    }

    fn is_running(&self) -> bool {
        lock(&self.connections).running
    }

    fn accepted(&self, stream: UnixStream) {
        if !self.is_running() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
        let Some(uid) = peer_uid(&stream) else {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        };
        if !(self.authorize)(uid) {
            eprintln!("hostd: refused a connection from uid {uid}");
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
        let _ = stream.set_nonblocking(false);
        let Ok(writer) = stream.try_clone() else {
            let _ = stream.shutdown(std::net::Shutdown::Both);
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
        *lock(&connection.on_close) = Some(Box::new(move || {
            if let Some(all) = connections.upgrade() {
                lock(&all).open.remove(&key);
            }
        }));
        let stopped = {
            let mut all = lock(&self.connections);
            if all.running {
                all.open.insert(key, connection.clone());
            }
            !all.running
        };
        if stopped {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            connection.close();
            return;
        }
        std::thread::spawn(move || connection.read_loop(stream));
    }
}

fn accept_loop(server: &Weak<HostServer>, listener: &UnixListener, wake: &UnixStream) {
    loop {
        let Some(server) = server.upgrade() else {
            drain_listener(listener);
            return;
        };
        if !server.is_running() {
            drain_listener(listener);
            return;
        }
        let mut pfds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: pfds is a valid array of two pollfds.
        let ready = unsafe { libc::poll(pfds.as_mut_ptr(), 2, 200) };
        if ready <= 0 {
            continue;
        }
        if pfds[1].revents != 0 {
            drain_listener(listener);
            return;
        }
        if pfds[0].revents == 0 {
            continue;
        }
        loop {
            if !server.is_running() {
                drain_listener(listener);
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    set_no_sigpipe(&stream);
                    server.accepted(stream);
                }
                Err(error) if retry_accept(&error) => continue,
                Err(_) => break,
            }
        }
    }
}

fn retry_accept(error: &io::Error) -> bool {
    use std::io::ErrorKind::{ConnectionAborted, ConnectionReset, Interrupted};
    matches!(
        error.kind(),
        Interrupted | ConnectionAborted | ConnectionReset
    )
}

fn drain_listener(listener: &UnixListener) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            Err(error) if retry_accept(&error) => continue,
            Err(_) => break,
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

fn broadcast(connections: &Mutex<Connections>, event: HostEvent) {
    let all: Vec<Arc<Connection>> = lock(connections).open.values().cloned().collect();
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
        lock(&self.listeners).insert(session, token);
        if self.closed.load(Ordering::SeqCst) {
            if let Some(orphan) = lock(&self.listeners).remove(&session) {
                handle.remove_listener(orphan);
            }
        }
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
