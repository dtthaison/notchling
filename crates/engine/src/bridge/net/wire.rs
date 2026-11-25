use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Mutex, TryLockError};
use std::time::Duration;

pub(crate) trait Wire: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
    fn write_all(&self, data: &[u8]) -> io::Result<()>;
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
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

    fn flush(&self, state: &mut TlsState) -> io::Result<()> {
        while state.conn.wants_write() {
            state.conn.write_tls(&mut &self.socket)?;
        }
        Ok(())
    }

    fn plaintext(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let mut state = lock(&self.state);
        loop {
            match state.conn.reader().read(buf) {
                Ok(n) => return Ok(Some(n)),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if state.pending.is_empty() {
                self.flush(&mut state)?;
                return Ok(None);
            }
            let TlsState { conn, pending } = &mut *state;
            let taken = conn.read_tls(&mut &pending[..])?;
            pending.drain(..taken);
            let processed = state.conn.process_new_packets();
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
            Err(TryLockError::WouldBlock) => None,
        };
        if let Some(mut state) = state {
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
