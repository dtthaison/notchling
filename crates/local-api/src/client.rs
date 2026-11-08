//! The caller's end: connect, `hello`, then calls; after [`Client::subscribe`], events too.
//!
//! Every call blocks until its reply or the client's timeout, so the app runs a client on a
//! background thread, never its UI thread; [`Client::into_events`] does that for an event stream.

use crate::framing::{write_line, FrameError, LineReader};
use crate::transport::{self, Stream};
use crate::wire::{
    ops, ApiError, ClientKind, DaemonStatus, Event, Hello, HelloReply, Incoming, Tier, API_VERSION,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// How long a call waits for its reply by default.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a call failed.
#[derive(Debug)]
pub enum ClientError {
    /// No daemon listens on the socket (missing, or a stale file nobody accepts on).
    NotRunning(io::Error),
    /// The daemon answered with an error.
    Api(ApiError),
    /// No reply in time.
    Timeout,
    /// The daemon closed the connection.
    Closed,
    /// A line that isn't the protocol.
    Protocol(String),
    /// This system has no transport yet (Windows in Milestone 2).
    Unsupported,
    Io(io::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::NotRunning(e) => write!(f, "the daemon isn't running ({e})"),
            ClientError::Api(e) => write!(f, "{e}"),
            ClientError::Timeout => write!(f, "the daemon didn't answer in time"),
            ClientError::Closed => write!(f, "the daemon closed the connection"),
            ClientError::Protocol(e) => write!(f, "the daemon sent something unexpected: {e}"),
            ClientError::Unsupported => {
                write!(f, "the local API is not supported on this system yet")
            }
            ClientError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<FrameError> for ClientError {
    fn from(e: FrameError) -> Self {
        match e {
            FrameError::TooLong => ClientError::Protocol("a line over the size cap".into()),
            FrameError::PartialAtEof => ClientError::Closed,
            FrameError::Io(e) => ClientError::Io(e),
        }
    }
}

/// Connects without saying hello: `NotRunning` when nobody listens.
pub fn connect_raw(socket: &Path) -> Result<Stream, ClientError> {
    transport::connect(socket).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NotRunning(e),
        io::ErrorKind::Unsupported => ClientError::Unsupported,
        _ => ClientError::Io(e),
    })
}

/// A connection that has said hello.
pub struct Client {
    stream: Stream,
    reader: LineReader<Stream>,
    next_id: u64,
    events: VecDeque<Event>,
    hello: HelloReply,
    timeout: Duration,
}

impl Client {
    /// Connects and says hello as `kind` with `version`, waiting up to [`CALL_TIMEOUT`].
    pub fn connect(socket: &Path, kind: ClientKind, version: &str) -> Result<Client, ClientError> {
        Client::connect_with_timeout(socket, kind, version, CALL_TIMEOUT)
    }

    pub fn connect_with_timeout(
        socket: &Path,
        kind: ClientKind,
        version: &str,
        timeout: Duration,
    ) -> Result<Client, ClientError> {
        let stream = connect_raw(socket)?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(ClientError::Io)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(ClientError::Io)?;
        let reader = LineReader::new(stream.try_clone().map_err(ClientError::Io)?);
        let mut client = Client {
            stream,
            reader,
            next_id: 1,
            events: VecDeque::new(),
            hello: HelloReply {
                api: API_VERSION,
                daemon: String::new(),
                tier: Tier::Agent,
            },
            timeout,
        };
        let args = serde_json::to_value(Hello {
            client: kind,
            version: version.to_string(),
            api: API_VERSION,
        })
        .map_err(|e| ClientError::Protocol(e.to_string()))?;
        client.hello = client.call_as(ops::HELLO, args)?;
        Ok(client)
    }

    /// What the daemon said to hello: its version and this connection's tier.
    pub fn hello(&self) -> &HelloReply {
        &self.hello
    }

    pub fn tier(&self) -> Tier {
        self.hello.tier
    }

    /// Calls an op and waits for its reply. Events that arrive meanwhile are kept for
    /// [`Client::next_event`].
    pub fn call(&mut self, op: &str, args: Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "id": id, "op": op, "args": args });
        write_line(&mut self.stream, &request).map_err(ClientError::Io)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.read_until(deadline)? {
                Incoming::Reply { id: got, result } if got == id => {
                    return result.map_err(ClientError::Api)
                }
                // A reply to a request this client gave up on.
                Incoming::Reply { .. } => {}
                Incoming::Event(event) => self.events.push_back(event),
            }
        }
    }

    /// [`Client::call`], read as `T`.
    pub fn call_as<T: DeserializeOwned>(
        &mut self,
        op: &str,
        args: Value,
    ) -> Result<T, ClientError> {
        let value = self.call(op, args)?;
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// `daemon.status`.
    pub fn status(&mut self) -> Result<DaemonStatus, ClientError> {
        self.call_as(ops::DAEMON_STATUS, json!({}))
    }

    /// `sessions.list`, read as the caller's row type (`notchling_engine::sessions::Agent`).
    pub fn sessions<T: DeserializeOwned>(&mut self) -> Result<Vec<T>, ClientError> {
        self.call_as(ops::SESSIONS_LIST, json!({}))
    }

    /// `events.subscribe`. Subscribe first, then list, so no change falls between the two.
    pub fn subscribe(&mut self, topics: &[&str]) -> Result<(), ClientError> {
        self.call(ops::EVENTS_SUBSCRIBE, json!({ "topics": topics }))
            .map(|_| ())
    }

    /// The next event, waiting up to `wait`; `None` when none came.
    pub fn next_event(&mut self, wait: Duration) -> Result<Option<Event>, ClientError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let deadline = Instant::now() + wait;
        loop {
            match self.read_until(deadline) {
                Ok(Incoming::Event(event)) => return Ok(Some(event)),
                Ok(Incoming::Reply { .. }) => {}
                Err(ClientError::Timeout) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    /// Moves the client to a thread that forwards its events; the channel ends with the error that
    /// ended the connection (or when the receiver is dropped, at the next event).
    pub fn into_events(mut self) -> Receiver<Result<Event, ClientError>> {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notchling-api-events".into())
            .spawn(move || loop {
                match self.next_event(Duration::from_secs(3600)) {
                    Ok(Some(event)) => {
                        if tx.send(Ok(event)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(Err(ClientError::Io(e)));
            return rx;
        }
        rx
    }

    fn read_until(&mut self, deadline: Instant) -> Result<Incoming, ClientError> {
        loop {
            match self.reader.next_line() {
                Ok(Some(line)) => return Incoming::parse(&line).map_err(ClientError::Protocol),
                Ok(None) => return Err(ClientError::Closed),
                Err(e) if e.is_timeout() => {
                    if Instant::now() >= deadline {
                        return Err(ClientError::Timeout);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}
