use crate::link::{await_ack, handshake, send_ack, start, Open};
use crate::tls::SERVER_NAME;
use crate::{allowed, client_config, server_config, Conn, Error, Identity, LinkEvents, Listener};
use crate::{Pins, Transport};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const CONNECT_WAIT: Duration = Duration::from_secs(5);
pub const MAX_LINKS: usize = 64;
const ACCEPT_POLL: Duration = Duration::from_millis(25);

pub struct TcpTls {
    identity: Identity,
    pins: Pins,
    loopback_ok: bool,
    links: Arc<AtomicUsize>,
}

struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl TcpTls {
    pub fn new(identity: Identity, pins: Pins, loopback_ok: bool) -> TcpTls {
        TcpTls {
            identity,
            pins,
            loopback_ok,
            links: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn pins(&self) -> &Pins {
        &self.pins
    }

    fn bind_ips(&self) -> Vec<IpAddr> {
        if self.loopback_ok {
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]
        } else {
            notchling_platform::net_if::tailscale_addrs()
        }
    }
}

fn not_tailscale(addr: &str) -> Error {
    Error::NotAllowed(format!("{addr} is not a Tailscale address"))
}

impl Transport for TcpTls {
    fn addrs(&self) -> Vec<String> {
        self.bind_ips().iter().map(IpAddr::to_string).collect()
    }

    fn dial(
        &self,
        addrs: &[String],
        port: u16,
        expect: &str,
        events: Arc<dyn LinkEvents>,
    ) -> Result<Arc<dyn Conn>, Error> {
        let config = client_config(&self.identity, expect)?;
        let mut last = Error::NotAllowed("the node has no address".into());
        for addr in addrs {
            let Ok(ip) = addr.parse::<IpAddr>() else {
                last = not_tailscale(addr);
                continue;
            };
            if !allowed(&ip, self.loopback_ok) {
                last = not_tailscale(addr);
                continue;
            }
            let mut stream =
                match TcpStream::connect_timeout(&SocketAddr::new(ip, port), CONNECT_WAIT) {
                    Ok(stream) => stream,
                    Err(e) => {
                        last = Error::Io(e);
                        continue;
                    }
                };
            let name = rustls_pki_types::ServerName::try_from(SERVER_NAME)
                .map_err(|e| Error::Tls(e.to_string()))?;
            let mut conn: rustls::Connection =
                rustls::ClientConnection::new(config.clone(), name)?.into();
            let peer_fp = handshake(&mut conn, &mut stream)?;
            if peer_fp != expect {
                return Err(Error::Tls(
                    "the node's certificate is not the pinned one".into(),
                ));
            }
            let first = await_ack(&mut conn, &mut stream)?;
            let (conn, runner) = start(
                Open {
                    conn,
                    stream,
                    peer_fp,
                    first,
                    slot: None,
                },
                events,
            )?;
            if let Err(e) = std::thread::Builder::new()
                .name("cluster-link".into())
                .spawn(runner)
            {
                conn.close();
                return Err(Error::Io(e));
            }
            return Ok(conn);
        }
        Err(last)
    }

    fn listen(&self, port: u16, events: Arc<dyn LinkEvents>) -> Result<Listener, Error> {
        let config = server_config(&self.identity, self.pins.clone())?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut bound = Vec::new();
        let mut threads = Vec::new();
        for ip in self.bind_ips() {
            let listener = TcpListener::bind(SocketAddr::new(ip, port))?;
            listener.set_nonblocking(true)?;
            bound.push(listener.local_addr()?);
            let accept = Accept {
                config: config.clone(),
                pins: self.pins.clone(),
                loopback_ok: self.loopback_ok,
                links: self.links.clone(),
                events: events.clone(),
            };
            let stop = stop.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("cluster-accept".into())
                    .spawn(move || accept.run(listener, &stop))?,
            );
        }
        Ok(Listener::new(
            bound,
            Box::new(move || {
                stop.store(true, Ordering::SeqCst);
                for thread in threads {
                    let _ = thread.join();
                }
            }),
        ))
    }
}

struct Accept {
    config: Arc<rustls::ServerConfig>,
    pins: Pins,
    loopback_ok: bool,
    links: Arc<AtomicUsize>,
    events: Arc<dyn LinkEvents>,
}

impl Accept {
    fn run(self, listener: TcpListener, stop: &AtomicBool) {
        while !stop.load(Ordering::SeqCst) {
            let (stream, peer) = match listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL);
                    continue;
                }
                Err(_) => {
                    std::thread::sleep(ACCEPT_POLL * 4);
                    continue;
                }
            };
            if !allowed(&peer.ip(), self.loopback_ok) {
                continue;
            }
            if self.links.fetch_add(1, Ordering::SeqCst) >= MAX_LINKS {
                self.links.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let counted = Counted(self.links.clone());
            let (config, pins, events) =
                (self.config.clone(), self.pins.clone(), self.events.clone());
            let _ = std::thread::Builder::new()
                .name("cluster-link".into())
                .spawn(move || {
                    let _counted = counted;
                    let _ = serve_one(stream, config, pins, events);
                });
        }
    }
}

fn serve_one(
    mut stream: TcpStream,
    config: Arc<rustls::ServerConfig>,
    pins: Pins,
    events: Arc<dyn LinkEvents>,
) -> Result<(), Error> {
    stream.set_nonblocking(false)?;
    let mut conn: rustls::Connection = rustls::ServerConnection::new(config)?.into();
    let peer_fp = handshake(&mut conn, &mut stream)?;
    let Some(slot) = pins.admit(&peer_fp) else {
        conn.send_close_notify();
        let _ = conn.write_tls(&mut stream);
        return Err(Error::NotAllowed("this node is not pinned".into()));
    };
    send_ack(&mut conn, &mut stream)?;
    let (_conn, runner) = start(
        Open {
            conn,
            stream,
            peer_fp,
            first: Vec::new(),
            slot: Some(slot),
        },
        events,
    )?;
    runner();
    Ok(())
}
