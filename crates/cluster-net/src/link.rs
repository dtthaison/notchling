use crate::frame::{LineReader, Outbox, MAX_LINE, OUTBOX};
use crate::pin::Admit;
use crate::{fingerprint, Error};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const HANDSHAKE: Duration = Duration::from_secs(10);
pub const WRITE_WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(20);
const ACK: &[u8] = b"\n";

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub fn next_conn_id() -> u64 {
    NEXT_CONN.fetch_add(1, Ordering::SeqCst)
}

pub trait LinkEvents: Send + Sync {
    fn opened(&self, conn: Arc<dyn Conn>);
    fn line(&self, peer_fp: &str, line: Vec<u8>);
    fn closed(&self, peer_fp: &str, conn_id: u64);
}

pub trait Conn: Send + Sync {
    fn id(&self) -> u64;
    fn send(&self, line: Vec<u8>) -> Result<(), Error>;
    fn peer_fingerprint(&self) -> String;
    fn is_open(&self) -> bool;
    fn close(&self);
}

pub trait Transport: Send + Sync {
    fn addrs(&self) -> Vec<String>;
    fn dial(
        &self,
        addrs: &[String],
        port: u16,
        expect: &str,
        events: Arc<dyn LinkEvents>,
    ) -> Result<Arc<dyn Conn>, Error>;
    fn listen(&self, port: u16, events: Arc<dyn LinkEvents>) -> Result<Listener, Error>;
}

pub struct Listener {
    addrs: Vec<SocketAddr>,
    on_stop: Option<Box<dyn FnOnce() + Send>>,
}

impl Listener {
    pub(crate) fn new(addrs: Vec<SocketAddr>, on_stop: Box<dyn FnOnce() + Send>) -> Listener {
        Listener {
            addrs,
            on_stop: Some(on_stop),
        }
    }

    pub fn addrs(&self) -> Vec<SocketAddr> {
        self.addrs.clone()
    }

    pub fn ports(&self) -> Vec<u16> {
        self.addrs.iter().map(SocketAddr::port).collect()
    }

    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        if let Some(stop) = self.on_stop.take() {
            stop();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.halt();
    }
}

pub(crate) struct TlsConn {
    id: u64,
    peer_fp: String,
    outbox: Outbox,
    closed: Arc<AtomicBool>,
    stream: TcpStream,
}

impl Conn for TlsConn {
    fn id(&self) -> u64 {
        self.id
    }

    fn send(&self, line: Vec<u8>) -> Result<(), Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        if self.outbox.push(line) {
            Ok(())
        } else {
            self.close();
            Err(Error::Closed)
        }
    }

    fn peer_fingerprint(&self) -> String {
        self.peer_fp.clone()
    }

    fn is_open(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

pub(crate) fn handshake(
    conn: &mut rustls::Connection,
    stream: &mut TcpStream,
) -> Result<String, Error> {
    let deadline = Instant::now() + HANDSHAKE;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(WRITE_WAIT))?;
    while conn.is_handshaking() {
        if Instant::now() > deadline {
            return Err(Error::Tls("the handshake took too long".into()));
        }
        match conn.complete_io(stream) {
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(e) => return Err(Error::Tls(e.to_string())),
        }
    }
    let peer = conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .ok_or_else(|| Error::Tls("the peer sent no certificate".into()))?;
    Ok(fingerprint(peer.as_ref()))
}

fn flush(conn: &mut rustls::Connection, stream: &mut TcpStream) -> Result<(), Error> {
    while conn.wants_write() {
        conn.write_tls(stream)?;
    }
    Ok(())
}

fn read_some(
    conn: &mut rustls::Connection,
    stream: &mut TcpStream,
    plain: &mut Vec<u8>,
) -> Result<bool, Error> {
    match conn.read_tls(stream) {
        Ok(0) => return Err(Error::Closed),
        Ok(_) => {}
        Err(e) if timed_out(&e) => return Ok(false),
        Err(e) => return Err(Error::Io(e)),
    }
    let state = conn.process_new_packets()?;
    let mut want = state.plaintext_bytes_to_read();
    let mut buf = [0u8; 16 * 1024];
    while want > 0 {
        match conn.reader().read(&mut buf) {
            Ok(0) => return Err(Error::Closed),
            Ok(n) => {
                plain.extend_from_slice(&buf[..n]);
                want = want.saturating_sub(n);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => return Err(Error::Io(e)),
        }
    }
    if state.peer_has_closed() {
        return Err(Error::Closed);
    }
    Ok(true)
}

pub(crate) fn send_ack(conn: &mut rustls::Connection, stream: &mut TcpStream) -> Result<(), Error> {
    conn.writer().write_all(ACK)?;
    flush(conn, stream)
}

pub(crate) fn await_ack(
    conn: &mut rustls::Connection,
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Error> {
    let deadline = Instant::now() + HANDSHAKE;
    let mut plain = Vec::new();
    flush(conn, stream)?;
    while plain.is_empty() {
        if Instant::now() > deadline {
            return Err(Error::Tls("the node never confirmed the link".into()));
        }
        read_some(conn, stream, &mut plain)?;
        flush(conn, stream)?;
    }
    Ok(plain)
}

pub(crate) struct Open {
    pub conn: rustls::Connection,
    pub stream: TcpStream,
    pub peer_fp: String,
    pub first: Vec<u8>,
    pub slot: Option<Admit>,
}

pub(crate) type Runner = Box<dyn FnOnce() + Send>;

pub(crate) fn start(
    open: Open,
    events: Arc<dyn LinkEvents>,
) -> Result<(Arc<dyn Conn>, Runner), Error> {
    let (outbox, rx) = Outbox::new(OUTBOX);
    let closed = Arc::new(AtomicBool::new(false));
    let tls = Arc::new(TlsConn {
        id: next_conn_id(),
        peer_fp: open.peer_fp.clone(),
        outbox,
        closed: closed.clone(),
        stream: open.stream.try_clone()?,
    });
    let conn: Arc<dyn Conn> = tls.clone();
    events.opened(conn.clone());
    let id = tls.id;
    let runner: Runner = Box::new(move || {
        let peer = open.peer_fp.clone();
        let _slot = open.slot;
        serve(
            open.conn,
            open.stream,
            open.first,
            rx,
            &closed,
            events.as_ref(),
            &peer,
        );
        closed.store(true, Ordering::SeqCst);
        events.closed(&peer, id);
    });
    Ok((conn, runner))
}

fn serve(
    mut conn: rustls::Connection,
    mut stream: TcpStream,
    first: Vec<u8>,
    rx: Receiver<Vec<u8>>,
    closed: &AtomicBool,
    events: &dyn LinkEvents,
    peer: &str,
) {
    conn.set_buffer_limit(None);
    if stream.set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let mut reader = LineReader::new(MAX_LINE);
    let mut plain = first;
    while let Ok(lines) = reader.push(&plain) {
        for line in lines {
            events.line(peer, line);
        }
        plain.clear();
        if closed.load(Ordering::SeqCst) {
            break;
        }
        let mut wrote = false;
        while let Ok(line) = rx.try_recv() {
            if conn.writer().write_all(&line).is_err() {
                return finish(&mut conn, &mut stream);
            }
            wrote = true;
        }
        if (wrote || conn.wants_write()) && flush(&mut conn, &mut stream).is_err() {
            break;
        }
        if read_some(&mut conn, &mut stream, &mut plain).is_err() {
            break;
        }
    }
    finish(&mut conn, &mut stream);
}

fn finish(conn: &mut rustls::Connection, stream: &mut TcpStream) {
    conn.send_close_notify();
    let _ = flush(conn, stream);
    let _ = stream.shutdown(Shutdown::Both);
}
