//! The byte stream under one connection: plain TCP (the tunnel origin) or TLS (the LAN port).
//!
//! One thread reads, another writes. For TLS the socket read happens without the lock, and only
//! feeding the bytes to rustls, reading plaintext and writing records take it, so a reader blocked
//! on a quiet phone never holds up a write. The TLS handshake runs inside the first reads.
//!
//! A TLS read returns after every socket read: when the bytes held no plaintext yet (a handshake,
//! or a record still arriving) it fails with `Interrupted`, so the caller checks its own deadline
//! before reading again, and a client sending a byte at a time can't outlast it.
//!
//! [`Wire::shutdown`] never waits: it closes the socket, which wakes a write blocked on a phone
//! that stopped reading, and sends TLS's `close_notify` only when no write holds the session.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Mutex, TryLockError};
use std::time::Duration;

pub(crate) trait Wire: Send + Sync {
    /// Plaintext bytes; 0 at the end. One reader at a time. `Interrupted`: bytes came but no
    /// plaintext yet, so call again.
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
    fn write_all(&self, data: &[u8]) -> io::Result<()>;
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    /// Ends both directions; a blocked read or write returns. Never waits.
    fn shutdown(&self);
}

pub(crate) struct PlainWire {
    reader: TcpStream,
    writer: Mutex<TcpStream>,
}

impl PlainWire {
    pub fn new(stream: TcpStream) -> io::Result<PlainWire> {
        Ok(PlainWire {
            writer: Mutex::new(stream.try_clone()?),
            reader: stream,
        })
    }
}

impl Wire for PlainWire {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.reader).read(buf)
    }
    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        lock(&self.writer).write_all(data)
    }
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.set_read_timeout(timeout)
    }
    fn shutdown(&self) {
        let _ = self.reader.shutdown(Shutdown::Both);
    }
}

struct TlsState {
    conn: rustls::ServerConnection,
    /// Received TLS bytes rustls hasn't taken yet.
    pending: Vec<u8>,
}

pub(crate) struct TlsWire {
    socket: TcpStream,
    state: Mutex<TlsState>,
}

impl TlsWire {
    pub fn new(stream: TcpStream, conn: rustls::ServerConnection) -> TlsWire {
        TlsWire {
            socket: stream,
            state: Mutex::new(TlsState {
                conn,
                pending: Vec::new(),
            }),
        }
    }

    /// Writes every TLS record rustls has ready.
    fn flush(&self, state: &mut TlsState) -> io::Result<()> {
        while state.conn.wants_write() {
            state.conn.write_tls(&mut &self.socket)?;
        }
        Ok(())
    }

    /// Plaintext from what has arrived, without reading the socket; `None` when there is none yet.
    fn plaintext(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let mut state = lock(&self.state);
        loop {
            match state.conn.reader().read(buf) {
                Ok(n) => return Ok(Some(n)),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if state.pending.is_empty() {
                // The handshake's answers go out before the next socket read.
                self.flush(&mut state)?;
                return Ok(None);
            }
            let TlsState { conn, pending } = &mut *state;
            let taken = conn.read_tls(&mut &pending[..])?;
            pending.drain(..taken);
            let processed = state.conn.process_new_packets();
            // An alert rustls queued for a bad handshake goes out before the error.
            let _ = self.flush(&mut state);
            if let Err(e) = processed {
                return Err(io::Error::new(io::ErrorKind::InvalidData, e));
            }
            if taken == 0 && state.conn.wants_read() {
                return Err(io::Error::other("TLS buffer full"));
            }
        }
    }
}

impl Wire for TlsWire {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(n) = self.plaintext(buf)? {
            return Ok(n);
        }
        let mut raw = [0u8; 16 * 1024];
        let n = (&self.socket).read(&mut raw)?;
        if n == 0 {
            return Ok(0);
        }
        lock(&self.state).pending.extend_from_slice(&raw[..n]);
        match self.plaintext(buf)? {
            Some(n) => Ok(n),
            None => Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "no plaintext yet",
            )),
        }
    }

    /// rustls takes at most its buffer limit (64 KB) of plaintext before its records are written,
    /// so a large frame goes in pieces.
    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        let mut state = lock(&self.state);
        let mut rest = data;
        while !rest.is_empty() {
            let n = state.conn.writer().write(rest)?;
            self.flush(&mut state)?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            rest = &rest[n..];
        }
        self.flush(&mut state)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }

    fn shutdown(&self) {
        let state = match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            // A write holds it, maybe blocked on a phone that stopped reading: no goodbye then.
            Err(TryLockError::WouldBlock) => None,
        };
        if let Some(mut state) = state {
            // Only what fits now: the socket goes non-blocking, and it closes next anyway.
            state.conn.send_close_notify();
            if self.socket.set_nonblocking(true).is_ok() {
                let _ = self.flush(&mut state);
            }
        }
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
