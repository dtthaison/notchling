use crate::frame::{LineReader, Outbox, MAX_LINE, OUTBOX};
use crate::link::next_conn_id;
use crate::pin::Admit;
use crate::{Conn, Error, LinkEvents, Listener, Pins, Transport};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Clone)]
struct Entry {
    fp: String,
    pins: Pins,
    events: Arc<dyn LinkEvents>,
    token: u64,
}

#[derive(Default)]
pub struct MemNet {
    listeners: Mutex<BTreeMap<(String, u16), Entry>>,
    next: AtomicU64,
}

impl MemNet {
    pub fn new() -> Arc<MemNet> {
        Arc::new(MemNet::default())
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<(String, u16), Entry>> {
        self.listeners.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct MemTransport {
    net: Arc<MemNet>,
    fp: String,
    pins: Pins,
    name: String,
}

impl MemTransport {
    pub fn on(net: &Arc<MemNet>, identity_fp: &str, pins: Pins) -> MemTransport {
        MemTransport {
            net: net.clone(),
            fp: identity_fp.to_string(),
            pins,
            name: format!("mem-{}", identity_fp.chars().take(16).collect::<String>()),
        }
    }

    pub fn pins(&self) -> &Pins {
        &self.pins
    }
}

struct Pair {
    closed: AtomicBool,
    closing: AtomicBool,
    drained: AtomicUsize,
    slot: Mutex<Option<Admit>>,
}

impl Pair {
    fn finish(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst) || self.is_closed()
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

struct MemConn {
    id: u64,
    peer_fp: String,
    outbox: Outbox,
    pair: Arc<Pair>,
}

impl Conn for MemConn {
    fn id(&self) -> u64 {
        self.id
    }

    fn send(&self, line: Vec<u8>) -> Result<(), Error> {
        if self.pair.is_closing() {
            return Err(Error::Closed);
        }
        if self.outbox.push(line) {
            Ok(())
        } else {
            self.pair.close();
            Err(Error::Closed)
        }
    }

    fn peer_fingerprint(&self) -> String {
        self.peer_fp.clone()
    }

    fn is_open(&self) -> bool {
        !self.pair.is_closing()
    }

    fn close(&self) {
        self.pair.finish();
    }
}

struct Leg {
    rx: Receiver<Vec<u8>>,
    from_fp: String,
    to: Arc<dyn LinkEvents>,
    to_conn: u64,
    to_peer_fp: String,
    pair: Arc<Pair>,
}

impl Leg {
    fn run(self) {
        let mut reader = LineReader::new(MAX_LINE);
        let mut drained = false;
        while !self.pair.is_closed() {
            match self.rx.recv_timeout(Duration::from_millis(20)) {
                Ok(bytes) => match reader.push(&bytes) {
                    Ok(lines) => {
                        for line in lines {
                            if self.pair.is_closed() {
                                break;
                            }
                            self.to.line(&self.from_fp, line);
                        }
                    }
                    Err(_) => self.pair.close(),
                },
                Err(RecvTimeoutError::Timeout) if self.pair.is_closing() && !drained => {
                    drained = true;
                    if self.pair.drained.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                        self.pair.close();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.pair.close(),
            }
        }
        self.to.closed(&self.to_peer_fp, self.to_conn);
    }
}

fn spawn_leg(leg: Leg) -> Result<(), Error> {
    std::thread::Builder::new()
        .name("cluster-mem".into())
        .spawn(move || leg.run())?;
    Ok(())
}

impl Transport for MemTransport {
    fn addrs(&self) -> Vec<String> {
        vec![self.name.clone()]
    }

    fn dial(
        &self,
        addrs: &[String],
        port: u16,
        expect: &str,
        events: Arc<dyn LinkEvents>,
    ) -> Result<Arc<dyn Conn>, Error> {
        let entry = addrs
            .iter()
            .find_map(|a| self.net.lock().get(&(a.clone(), port)).cloned())
            .ok_or_else(|| Error::Io(std::io::ErrorKind::ConnectionRefused.into()))?;
        if entry.fp != expect {
            return Err(Error::Tls(
                "the node's certificate is not the pinned one".into(),
            ));
        }
        let slot = entry
            .pins
            .admit(&self.fp)
            .ok_or_else(|| Error::Tls("this node is not pinned".into()))?;
        let pair = Arc::new(Pair {
            closed: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            drained: AtomicUsize::new(0),
            slot: Mutex::new(Some(slot)),
        });
        let (out_c, rx_c) = Outbox::new(OUTBOX);
        let (out_s, rx_s) = Outbox::new(OUTBOX);
        let client = Arc::new(MemConn {
            id: next_conn_id(),
            peer_fp: entry.fp.clone(),
            outbox: out_c,
            pair: pair.clone(),
        });
        let server = Arc::new(MemConn {
            id: next_conn_id(),
            peer_fp: self.fp.clone(),
            outbox: out_s,
            pair: pair.clone(),
        });
        entry.events.opened(server.clone());
        events.opened(client.clone());
        spawn_leg(Leg {
            rx: rx_c,
            from_fp: self.fp.clone(),
            to: entry.events.clone(),
            to_conn: server.id,
            to_peer_fp: self.fp.clone(),
            pair: pair.clone(),
        })?;
        spawn_leg(Leg {
            rx: rx_s,
            from_fp: entry.fp.clone(),
            to: events,
            to_conn: client.id,
            to_peer_fp: entry.fp.clone(),
            pair,
        })?;
        Ok(client)
    }

    fn listen(&self, port: u16, events: Arc<dyn LinkEvents>) -> Result<Listener, Error> {
        let key = (self.name.clone(), port);
        let token = self.net.next.fetch_add(1, Ordering::SeqCst);
        {
            let mut listeners = self.net.lock();
            if listeners.contains_key(&key) {
                return Err(Error::Io(std::io::ErrorKind::AddrInUse.into()));
            }
            listeners.insert(
                key.clone(),
                Entry {
                    fp: self.fp.clone(),
                    pins: self.pins.clone(),
                    events,
                    token,
                },
            );
        }
        let net = self.net.clone();
        Ok(Listener::new(
            vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
            Box::new(move || {
                let mut listeners = net.lock();
                if listeners.get(&key).is_some_and(|e| e.token == token) {
                    listeners.remove(&key);
                }
            }),
        ))
    }
}
