use crate::host::HostedInfo;
use crate::wire::{self, Frame, FrameReader, SessionId};
use crate::HostError;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Output = Arc<dyn Fn(&[u8]) + Send + Sync>;
type EventHandler = Arc<dyn Fn(&Map<String, Value>) + Send + Sync>;
type ClosedHandler = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Shared {
    waiters: Mutex<HashMap<i64, Sender<Map<String, Value>>>>,
    outputs: Mutex<HashMap<SessionId, Output>>,
    on_event: Mutex<Option<EventHandler>>,
    on_closed: Mutex<Option<ClosedHandler>>,
    closed: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct HostClient {
    writer: Mutex<UnixStream>,
    next_id: AtomicI64,
    shared: Arc<Shared>,
}

impl HostClient {
    pub fn connect(path: &Path, timeout: Duration) -> Result<HostClient, HostError> {
        let stream = UnixStream::connect(path).map_err(|_| HostError::ConnectFailed)?;
        crate::server::set_no_sigpipe(&stream);
        let _ = stream.set_write_timeout(Some(timeout.max(Duration::from_millis(1))));
        let reader = stream.try_clone().map_err(|e| HostError::socket(&e))?;
        let shared = Arc::new(Shared::default());
        let for_reader = shared.clone();
        std::thread::spawn(move || read_loop(reader, &for_reader));
        Ok(HostClient {
            writer: Mutex::new(stream),
            next_id: AtomicI64::new(1),
            shared,
        })
    }

    pub fn is_connected(&self) -> bool {
        !self.shared.closed.load(Ordering::SeqCst)
    }

    pub fn on_event(&self, handler: impl Fn(&Map<String, Value>) + Send + Sync + 'static) {
        *lock(&self.shared.on_event) = Some(Arc::new(handler));
    }

    pub fn on_closed(&self, handler: impl FnOnce() + Send + 'static) {
        if self.shared.closed.load(Ordering::SeqCst) {
            handler();
            return;
        }
        *lock(&self.shared.on_closed) = Some(Box::new(handler));
    }

    pub fn close(&self) {
        let writer = lock(&self.writer);
        let _ = writer.shutdown(std::net::Shutdown::Both);
        drop(writer);
        finish(&self.shared);
    }

    fn send(&self, bytes: &[u8]) -> Result<(), HostError> {
        let mut writer = lock(&self.writer);
        writer.write_all(bytes).map_err(|e| HostError::socket(&e))
    }

    pub fn request(
        &self,
        op: &str,
        params: Map<String, Value>,
        timeout: Duration,
    ) -> Result<Map<String, Value>, HostError> {
        if self.shared.closed.load(Ordering::SeqCst) {
            return Err(HostError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        lock(&self.shared.waiters).insert(id, tx);
        let mut message = params;
        message.insert("id".into(), Value::from(id));
        message.insert("op".into(), Value::from(op));
        if let Err(error) = self.send(&wire::json(message)) {
            lock(&self.shared.waiters).remove(&id);
            return Err(error);
        }
        let reply = match rx.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.shared.waiters).remove(&id);
                return Err(HostError::Timeout);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(HostError::Closed),
        };
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            let why = reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Lỗi không rõ.");
            return Err(HostError::Refused(why.to_string()));
        }
        Ok(match reply.get("data") {
            Some(Value::Object(data)) => data.clone(),
            _ => Map::new(),
        })
    }

    pub fn sessions(&self) -> Result<Vec<HostedInfo>, HostError> {
        let reply = self.request("list", Map::new(), Duration::from_secs(5))?;
        Ok(reply
            .get("sessions")
            .and_then(Value::as_array)
            .map(|all| all.iter().filter_map(HostClient::info_from).collect())
            .unwrap_or_default())
    }

    pub fn info_from(value: &Value) -> Option<HostedInfo> {
        crate::ops::info_from(value)
    }

    pub fn attach(
        &self,
        session: SessionId,
        handler: impl Fn(&[u8]) + Send + Sync + 'static,
    ) -> Result<(), HostError> {
        lock(&self.shared.outputs).insert(session, Arc::new(handler));
        let result = self.request("attach", session_param(session), Duration::from_secs(5));
        if result.is_err() {
            lock(&self.shared.outputs).remove(&session);
        }
        result.map(|_| ())
    }

    pub fn detach(&self, session: SessionId) -> Result<(), HostError> {
        lock(&self.shared.outputs).remove(&session);
        self.request("detach", session_param(session), Duration::from_secs(5))
            .map(|_| ())
    }

    pub fn write(&self, session: SessionId, bytes: &[u8]) -> Result<(), HostError> {
        self.send(&wire::encode(&Frame::Data {
            session,
            bytes: bytes.to_vec(),
        }))
    }
}

fn session_param(session: SessionId) -> Map<String, Value> {
    let mut params = Map::new();
    params.insert("session".into(), Value::from(session.to_string()));
    params
}

fn read_loop(mut stream: UnixStream, shared: &Shared) {
    let mut reader = FrameReader::new();
    let mut buffer = vec![0u8; 65536];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for frame in reader.feed(&buffer[..count]) {
            dispatch(shared, frame);
        }
        if reader.failure().is_some() {
            break;
        }
    }
    finish(shared);
}

fn dispatch(shared: &Shared, frame: Frame) {
    match frame {
        Frame::Data { session, bytes } => {
            let handler = lock(&shared.outputs).get(&session).cloned();
            if let Some(handler) = handler {
                handler(&bytes);
            }
        }
        Frame::Json(message) => {
            if let Some(id) = wire::int(message.get("id")) {
                if let Some(waiter) = lock(&shared.waiters).remove(&id) {
                    let _ = waiter.send(message);
                }
            } else if message.contains_key("event") {
                let handler = lock(&shared.on_event).clone();
                if let Some(handler) = handler {
                    handler(&message);
                }
            }
        }
    }
}

fn finish(shared: &Shared) {
    if shared.closed.swap(true, Ordering::SeqCst) {
        return;
    }
    lock(&shared.waiters).clear();
    let handler = lock(&shared.on_closed).take();
    if let Some(handler) = handler {
        handler();
    }
}
