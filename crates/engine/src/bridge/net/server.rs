use super::address;
use super::core::Core;
use super::frame::{self, Event, FrameDecoder};
use super::http::{self, HeadParse, HeadReader, Upgrade};
use super::limiter;
use super::wire::{PlainWire, TlsWire, Wire};
use crate::bridge::{Connection, ConnectionIo};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub head_timeout: Duration,
    pub auth_timeout: Duration,
    pub ping_every: Duration,
    pub max_connections: usize,
    pub unauthenticated_per_address: usize,
    pub write_timeout: Duration,
    pub queue: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            head_timeout: Duration::from_millis(http::HEAD_TIMEOUT_MS as u64),
            auth_timeout: Duration::from_secs(10),
            ping_every: Duration::from_secs(20),
            max_connections: 64,
            unauthenticated_per_address: 4,
            write_timeout: Duration::from_secs(10),
            queue: 512,
        }
    }
}

#[derive(Clone)]
pub enum Kind {
    Lan(Arc<rustls::ServerConfig>),
    Tunnel,
}

#[derive(Default)]
struct Registry {
    next: AtomicU64,
    wires: Mutex<HashMap<u64, Entry>>,
}

struct Entry {
    wire: Arc<dyn Wire>,
    peer: Option<String>,
    opened: Instant,
    spoke: Arc<AtomicBool>,
    outbox: Option<Arc<Outbox>>,
    connection: Option<Arc<Connection>>,
    shut: bool,
}

impl Entry {
    fn authenticated(&self) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|c| c.is_authenticated())
    }
}

impl Registry {
    fn insert(
        &self,
        wire: Arc<dyn Wire>,
        peer: Option<String>,
        limits: &Limits,
    ) -> Option<(u64, Arc<AtomicBool>)> {
        let mut wires = lock(&self.wires);
        if wires.len() >= limits.max_connections {
            return None;
        }
        if let Some(peer) = &peer {
            let waiting = wires
                .values()
                .filter(|e| e.peer.as_ref() == Some(peer) && !e.authenticated())
                .count();
            if waiting >= limits.unauthenticated_per_address {
                return None;
            }
        }
        let key = self.next.fetch_add(1, Ordering::SeqCst);
        let spoke = Arc::new(AtomicBool::new(false));
        wires.insert(
            key,
            Entry {
                wire,
                peer,
                opened: Instant::now(),
                spoke: spoke.clone(),
                outbox: None,
                connection: None,
                shut: false,
            },
        );
        Some((key, spoke))
    }

    fn attach(&self, key: u64, outbox: Arc<Outbox>, connection: Arc<Connection>) {
        if let Some(entry) = lock(&self.wires).get_mut(&key) {
            entry.outbox = Some(outbox);
            entry.connection = Some(connection);
        }
    }

    fn remove(&self, key: u64) {
        lock(&self.wires).remove(&key);
    }

    fn sweep(&self, auth_timeout: Duration, ping: bool) -> (Vec<Arc<dyn Wire>>, Vec<Arc<Outbox>>) {
        let mut close = Vec::new();
        let mut pings = Vec::new();
        for entry in lock(&self.wires).values_mut().filter(|e| !e.shut) {
            let dropped = entry.outbox.as_ref().is_some_and(|o| o.is_done());
            let silent = !entry.spoke.load(Ordering::SeqCst)
                && !entry.authenticated()
                && entry.opened.elapsed() >= auth_timeout;
            if dropped || silent {
                entry.shut = true;
                close.push(entry.wire.clone());
            } else if ping {
                pings.extend(entry.outbox.clone());
            }
        }
        (close, pings)
    }
}

pub struct Listener {
    port: u16,
    stop: Arc<AtomicBool>,
    registry: Arc<Registry>,
    threads: Vec<JoinHandle<()>>,
}

impl Listener {
    pub fn start(
        addr: SocketAddr,
        kind: Kind,
        core: Arc<Core>,
        limits: Limits,
    ) -> io::Result<Listener> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let registry = Arc::new(Registry::default());
        let accept = {
            let (stop, registry) = (stop.clone(), registry.clone());
            std::thread::Builder::new()
                .name(format!("bridge-accept-{port}"))
                .spawn(move || accept_loop(listener, kind, core, limits, stop, registry))?
        };
        let watch = {
            let (stop, registry) = (stop.clone(), registry.clone());
            std::thread::Builder::new()
                .name(format!("bridge-watch-{port}"))
                .spawn(move || watch_loop(limits, stop, registry))?
        };
        Ok(Listener {
            port,
            stop,
            registry,
            threads: vec![accept, watch],
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn sockets(&self) -> usize {
        lock(&self.registry.wires).len()
    }

    pub fn stop(mut self) {
        self.close();
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        let entries: Vec<Arc<dyn Wire>> = lock(&self.registry.wires)
            .values()
            .map(|e| e.wire.clone())
            .collect();
        for wire in entries {
            wire.shutdown();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.close();
    }
}

fn accept_loop(
    listener: TcpListener,
    kind: Kind,
    core: Arc<Core>,
    limits: Limits,
    stop: Arc<AtomicBool>,
    registry: Arc<Registry>,
) {
    while !stop.load(Ordering::SeqCst) {
        let (stream, peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let allowed = match kind {
            Kind::Lan(_) => address::is_allowed_ip(peer.ip()),
            Kind::Tunnel => peer.ip().to_canonical().is_loopback(),
        };
        if !allowed {
            continue;
        }
        let (kind, core, registry) = (kind.clone(), core.clone(), registry.clone());
        let spawned = std::thread::Builder::new()
            .name("bridge-phone".into())
            .spawn(move || serve(stream, peer, kind, core, limits, registry));
        if spawned.is_err() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn watch_loop(limits: Limits, stop: Arc<AtomicBool>, registry: Arc<Registry>) {
    let mut last_ping = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50).min(limits.ping_every));
        let ping = last_ping.elapsed() >= limits.ping_every;
        if ping {
            last_ping = Instant::now();
        }
        let (close, pings) = registry.sweep(limits.auth_timeout, ping);
        for wire in close {
            wire.shutdown();
        }
        for outbox in pings {
            outbox.ping_tick();
        }
    }
}

fn serve(
    stream: TcpStream,
    peer: SocketAddr,
    kind: Kind,
    core: Arc<Core>,
    limits: Limits,
    registry: Arc<Registry>,
) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(limits.write_timeout));
    let wire: Arc<dyn Wire> = match &kind {
        Kind::Lan(tls) => match rustls::ServerConnection::new(tls.clone()) {
            Ok(conn) => Arc::new(TlsWire::new(stream, conn)),
            Err(_) => return,
        },
        Kind::Tunnel => match PlainWire::new(stream) {
            Ok(wire) => Arc::new(wire),
            Err(_) => return,
        },
    };
    let lan_peer = match kind {
        Kind::Lan(_) => Some(limiter::key(&address::canonical(peer.ip()))),
        Kind::Tunnel => None,
    };
    let Some((key, spoke)) = registry.insert(wire.clone(), lan_peer, &limits) else {
        if matches!(kind, Kind::Tunnel) {
            let _ = wire.write_all(&http::response(503, "busy\n", &[]));
        }
        wire.shutdown();
        return;
    };
    run(&wire, peer, &kind, &core, limits, &registry, (key, &spoke));
    registry.remove(key);
}

fn run(
    wire: &Arc<dyn Wire>,
    peer: SocketAddr,
    kind: &Kind,
    core: &Arc<Core>,
    limits: Limits,
    registry: &Registry,
    (key, spoke): (u64, &AtomicBool),
) {
    let Some((head, leftover)) = read_head(wire.as_ref(), limits.head_timeout) else {
        wire.shutdown();
        return;
    };
    let upgrade = match kind {
        Kind::Lan(_) => http::lan_upgrade(&head, &core.gate),
        Kind::Tunnel => http::tunnel_upgrade(&head, &core.gate),
    };
    let tunnel = match upgrade {
        Upgrade::Refuse { response, .. } => {
            let _ = wire.write_all(&response);
            wire.shutdown();
            return;
        }
        Upgrade::Accept { response, tunnel } => {
            if wire.write_all(&response).is_err() {
                wire.shutdown();
                return;
            }
            tunnel
        }
    };
    let _ = wire.set_read_timeout(Some(limits.ping_every * 3));
    let outbox = Outbox::start(wire.clone(), limits.queue);
    let address = tunnel
        .clone()
        .unwrap_or_else(|| address::canonical(peer.ip()));
    let io: Arc<dyn ConnectionIo> = outbox.clone();
    let connection = core.connect(&address, tunnel.is_some(), io);
    registry.attach(key, outbox.clone(), connection.clone());
    frames(
        wire.as_ref(),
        &leftover,
        (&connection, spoke),
        &outbox,
        core,
    );
    connection.close();
    core.closed(&connection);
}

fn read_head(wire: &dyn Wire, timeout: Duration) -> Option<(http::HttpHead, Vec<u8>)> {
    let deadline = Instant::now() + timeout;
    let mut reader = HeadReader::new(0);
    let mut buf = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || wire.set_read_timeout(Some(left)).is_err() {
            let _ = wire.write_all(&http::response(408, "request timeout\n", &[]));
            return None;
        }
        let n = match wire.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => return None,
        };
        match reader.feed(&buf[..n]) {
            HeadParse::NeedMore => {}
            HeadParse::Failure(status, reason) => {
                let _ = wire.write_all(&http::response(status, &format!("{reason}\n"), &[]));
                return None;
            }
            HeadParse::Complete(head, leftover) => return Some((head, leftover)),
        }
    }
}

fn frames(
    wire: &dyn Wire,
    leftover: &[u8],
    (connection, spoke): (&Arc<Connection>, &AtomicBool),
    outbox: &Outbox,
    core: &Arc<Core>,
) {
    let handlers = Arc::new(Handlers::default());
    let mut decoder = FrameDecoder::default();
    let mut events = decoder.feed(leftover);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        for event in events.drain(..) {
            match event {
                Event::Text(text) => {
                    spoke.store(true, Ordering::SeqCst);
                    handlers.run(core, connection, text);
                }
                Event::Binary(_) => {}
                Event::Ping(payload) => outbox.push(Out::Raw(frame::encode(frame::PONG, &payload))),
                Event::Pong(_) => outbox.missed.store(0, Ordering::SeqCst),
                Event::Close(_) => {
                    outbox.close_with(1000);
                    return;
                }
                Event::Failure(code, _) => {
                    outbox.close_with(code);
                    return;
                }
            }
            if connection.is_closing() || outbox.is_done() {
                return;
            }
        }
        match wire.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => events = decoder.feed(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[derive(Default)]
struct Handlers {
    running: Mutex<usize>,
    done: Condvar,
}

const MAX_HANDLERS: usize = 8;

impl Handlers {
    fn run(self: &Arc<Self>, core: &Arc<Core>, connection: &Arc<Connection>, text: String) {
        {
            let mut running = lock(&self.running);
            while *running >= MAX_HANDLERS {
                running = self.done.wait(running).unwrap_or_else(|e| e.into_inner());
            }
            *running += 1;
        }
        let (me, core2, connection2) = (self.clone(), core.clone(), connection.clone());
        let spawned = std::thread::Builder::new()
            .name("bridge-frame".into())
            .spawn(move || {
                core2.handle(&connection2, &text);
                me.finished();
            });
        if spawned.is_err() {
            self.finished();
            connection.close();
        }
    }

    fn finished(&self) {
        *lock(&self.running) -= 1;
        self.done.notify_one();
    }
}

type Sent = Box<dyn FnOnce() + Send>;

enum Out {
    Text(String),
    Binary(Vec<u8>, Sent),
    Raw(Vec<u8>),
    Close(u16),
}

struct Outbox {
    tx: SyncSender<Out>,
    done: Arc<AtomicBool>,
    missed: AtomicU32,
}

impl Outbox {
    fn start(wire: Arc<dyn Wire>, queue: usize) -> Arc<Outbox> {
        let (tx, rx) = mpsc::sync_channel(queue);
        let done = Arc::new(AtomicBool::new(false));
        let (w, d) = (wire.clone(), done.clone());
        let spawned = std::thread::Builder::new()
            .name("bridge-phone-writer".into())
            .spawn(move || write_loop(rx, w, d));
        if spawned.is_err() {
            done.store(true, Ordering::SeqCst);
            wire.shutdown();
        }
        Arc::new(Outbox {
            tx,
            done,
            missed: AtomicU32::new(0),
        })
    }

    fn push(&self, out: Out) {
        match self.tx.try_send(out) {
            Ok(()) => {}
            Err(TrySendError::Full(out)) | Err(TrySendError::Disconnected(out)) => {
                if let Out::Binary(_, sent) = out {
                    sent();
                }
                self.drop_now();
            }
        }
    }

    fn close_with(&self, code: u16) {
        self.push(Out::Close(code));
    }

    fn drop_now(&self) {
        self.done.store(true, Ordering::SeqCst);
    }

    fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    fn ping_tick(&self) {
        if self.missed.fetch_add(1, Ordering::SeqCst) >= 2 {
            self.drop_now();
            return;
        }
        self.push(Out::Raw(frame::encode(frame::PING, &[])));
    }
}

impl ConnectionIo for Outbox {
    fn send_text(&self, text: String) {
        self.push(Out::Text(text));
    }
    fn send_binary(&self, data: Vec<u8>, sent: Sent) {
        self.push(Out::Binary(data, sent));
    }
    fn close(&self) {
        self.close_with(1000);
    }
}

fn write_loop(rx: Receiver<Out>, wire: Arc<dyn Wire>, done: Arc<AtomicBool>) {
    while !done.load(Ordering::SeqCst) {
        let out = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(out) => out,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let result = match out {
            Out::Text(text) => wire.write_all(&frame::encode(frame::TEXT, text.as_bytes())),
            Out::Binary(data, sent) => {
                let r = wire.write_all(&frame::encode(frame::BINARY, &data));
                sent();
                r
            }
            Out::Raw(bytes) => wire.write_all(&bytes),
            Out::Close(code) => {
                let _ = wire.write_all(&frame::close_frame(code));
                break;
            }
        };
        if result.is_err() {
            break;
        }
    }
    done.store(true, Ordering::SeqCst);
    wire.shutdown();
    while let Ok(out) = rx.try_recv() {
        if let Out::Binary(_, sent) = out {
            sent();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "server_tests.rs"]
pub(crate) mod tests;
