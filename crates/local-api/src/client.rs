use crate::framing::{write_line, FrameError, LineReader};
use crate::peer::{same_user, PeerInfo};
use crate::spawn::spawn_allowed;
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

pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum ClientError {
    NotRunning(io::Error),
    Api(ApiError),
    Timeout,
    Closed,
    Protocol(String),
    Unsupported,
    UntrustedServer(String),
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
            ClientError::UntrustedServer(why) => {
                write!(
                    f,
                    "the socket isn't served by a trusted notchling daemon: {why}"
                )
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

pub fn connect_raw(socket: &Path) -> Result<Stream, ClientError> {
    transport::connect(socket).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NotRunning(e),
        io::ErrorKind::Unsupported => ClientError::Unsupported,
        _ => ClientError::Io(e),
    })
}

pub fn server_trusted(server: &PeerInfo, my_uid: u32) -> Result<(), String> {
    if !same_user(server.uid, my_uid) {
        return Err(format!("it runs as user {}", server.uid));
    }
    if cfg!(target_os = "macos") {
        let Some(exe) = &server.exe else {
            return Err("its executable can't be named".into());
        };
        if !spawn_allowed(exe) {
            return Err(format!("{} is not a notchling daemon", exe.display()));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectOptions {
    pub timeout: Duration,
    pub check_server: bool,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            timeout: CALL_TIMEOUT,
            check_server: true,
        }
    }
}

pub struct Client {
    stream: Stream,
    reader: LineReader<Stream>,
    next_id: u64,
    events: VecDeque<Event>,
    hello: HelloReply,
    timeout: Duration,
}

impl Client {
    pub fn connect(socket: &Path, kind: ClientKind, version: &str) -> Result<Client, ClientError> {
        Client::connect_with_timeout(socket, kind, version, CALL_TIMEOUT)
    }

    pub fn connect_with_timeout(
        socket: &Path,
        kind: ClientKind,
        version: &str,
        timeout: Duration,
    ) -> Result<Client, ClientError> {
        let options = ConnectOptions {
            timeout,
            ..ConnectOptions::default()
        };
        Client::connect_with(socket, kind, version, &options)
    }

    pub fn connect_with(
        socket: &Path,
        kind: ClientKind,
        version: &str,
        options: &ConnectOptions,
    ) -> Result<Client, ClientError> {
        let timeout = options.timeout;
        let stream = connect_raw(socket)?;
        if options.check_server {
            let server = transport::peer(&stream).map_err(|e| {
                ClientError::UntrustedServer(format!("its process can't be identified ({e})"))
            })?;
            server_trusted(&server, crate::peer::current_uid())
                .map_err(ClientError::UntrustedServer)?;
        }
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

    pub fn hello(&self) -> &HelloReply {
        &self.hello
    }

    pub fn tier(&self) -> Tier {
        self.hello.tier
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

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
                Incoming::Reply { .. } => {}
                Incoming::Event(event) => self.events.push_back(event),
            }
        }
    }

    pub fn call_as<T: DeserializeOwned>(
        &mut self,
        op: &str,
        args: Value,
    ) -> Result<T, ClientError> {
        let value = self.call(op, args)?;
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    pub fn status(&mut self) -> Result<DaemonStatus, ClientError> {
        self.call_as(ops::DAEMON_STATUS, json!({}))
    }

    pub fn sessions<T: DeserializeOwned>(&mut self) -> Result<Vec<T>, ClientError> {
        self.call_as(ops::SESSIONS_LIST, json!({}))
    }

    pub fn subscribe(&mut self, topics: &[&str]) -> Result<(), ClientError> {
        self.call(ops::EVENTS_SUBSCRIBE, json!({ "topics": topics }))
            .map(|_| ())
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn server(uid: u32, exe: Option<PathBuf>) -> PeerInfo {
        PeerInfo {
            uid,
            pid: 1,
            exe,
            generation: Some(1),
        }
    }

    #[test]
    fn a_server_of_another_user_is_never_trusted() {
        let err = server_trusted(&server(0, None), 501).unwrap_err();
        assert!(err.contains("user 0"), "{err}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_server_must_be_a_notchling_in_a_bundle_or_target_folder() {
        let dir = crate::testutil::TempDir::new("trust");
        let make = |rel: &str| {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"bin").unwrap();
            path
        };
        let me = crate::peer::current_uid();
        let bundled = make("Notchling.app/Contents/MacOS/notchling");
        let built = make("repo/target/debug/notchling");
        let stranger = make("bin/notchling");
        let renamed = make("repo/target/debug/other");
        assert!(server_trusted(&server(me, Some(bundled)), me).is_ok());
        assert!(server_trusted(&server(me, Some(built)), me).is_ok());
        assert!(server_trusted(&server(me, Some(stranger)), me).is_err());
        assert!(server_trusted(&server(me, Some(renamed)), me).is_err());
        assert!(server_trusted(&server(me, None), me).is_err());
    }
}
