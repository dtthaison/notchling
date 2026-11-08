//! Fakes for the bridge tests (Swift `BridgeTestSupport`): a recording connection, a canned
//! backend, in-memory devices, codes, limiter and audit, and a rig wiring them to a real router.

use super::backend::{
    Audit, AuditEntry, BridgeBackend, Devices, PairedDevice, PairingCodes, RateLimiter,
};
use super::connection::{lock, Connection, ConnectionIo};
use super::hub::Hub;
use super::model::*;
use super::router::{Router, RouterParts};
use super::wire::BridgeFailure;
use crate::sessions::testutil::TempDir;
use notchling_protocol::generated as wire;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 2026-11-01 09:44:25 UTC, the fixtures' "now" (Swift `fixtureNow`).
pub const NOW_MS: f64 = 1_793_541_465_000.0;

type Done = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct RecordingIo {
    texts: Mutex<Vec<String>>,
    binaries: Mutex<Vec<Vec<u8>>>,
    closed: AtomicBool,
    pub hold_binary: AtomicBool,
    held: Mutex<Vec<Done>>,
}

impl RecordingIo {
    pub fn texts(&self) -> Vec<String> {
        lock(&self.texts).clone()
    }
    pub fn binaries(&self) -> Vec<Vec<u8>> {
        lock(&self.binaries).clone()
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub fn json(&self, index: usize) -> Value {
        serde_json::from_str(&self.texts()[index]).expect("json")
    }
    pub fn last_json(&self) -> Value {
        let texts = self.texts();
        serde_json::from_str(texts.last().expect("a reply")).expect("json")
    }
    pub fn release_binary(&self) {
        let pending: Vec<Done> = std::mem::take(&mut *lock(&self.held));
        pending.into_iter().for_each(|done| done());
    }
}

impl ConnectionIo for RecordingIo {
    fn send_text(&self, text: String) {
        lock(&self.texts).push(text);
    }
    fn send_binary(&self, data: Vec<u8>, sent: Done) {
        lock(&self.binaries).push(data);
        if self.hold_binary.load(Ordering::SeqCst) {
            lock(&self.held).push(sent);
        } else {
            sent();
        }
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

pub fn error_code(reply: &Value) -> Option<&str> {
    reply.get("error")?.get("code")?.as_str()
}

pub fn is_ok(reply: &Value) -> bool {
    reply.get("ok").and_then(Value::as_bool) == Some(true)
}

pub fn data(reply: &Value) -> Value {
    reply.get("data").cloned().unwrap_or(Value::Null)
}

/// The canned backend; every field is public so a test sets what it needs.
#[derive(Default)]
pub struct FakeState {
    pub sessions: Vec<Session>,
    pub chat: Vec<ChatMessage>,
    pub prompts: HashMap<String, PendingPrompt>,
    pub refuse_approval: bool,
    pub send_delay_ms: u64,
    pub phone_error: Option<BridgeFailure>,
    pub commands: Vec<Command>,
    pub viz: Option<BridgeViz>,
    pub takeover: Option<Result<TakeoverOutcome, BridgeFailure>>,
    pub sent: Vec<(String, String)>,
    pub responses: Vec<(String, PromptResponse)>,
    pub new_sessions: Vec<(PathBuf, Option<String>)>,
    pub phone_starts: Vec<PhoneOptions>,
    pub interrupted: Vec<String>,
    pub takeovers: Vec<(String, bool)>,
    pub approval_calls: usize,
    pub session_calls: usize,
}

#[derive(Default)]
pub struct FakeBackend {
    pub state: Mutex<FakeState>,
}

impl FakeBackend {
    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut lock(&self.state))
    }
}

pub const JPEG: [u8; 4] = [0xFF, 0xD8, 0xFF, 0xD9];

impl BridgeBackend for FakeBackend {
    fn sessions(&self) -> Vec<Session> {
        self.with(|s| {
            s.session_calls += 1;
            s.sessions.clone()
        })
    }
    fn chat_messages(&self, _: &str, _: Option<&str>) -> Result<Vec<ChatMessage>, BridgeFailure> {
        Ok(self.with(|s| s.chat.clone()))
    }
    fn chat_image(&self, _: &str, _: &str, _: i64) -> Result<Vec<u8>, BridgeFailure> {
        Ok(JPEG.to_vec())
    }
    fn commands(&self, _: &str) -> Vec<Command> {
        self.with(|s| s.commands.clone())
    }
    fn viz(&self, _: &str, include_entity: bool) -> Result<BridgeViz, BridgeFailure> {
        let canned = self.with(|s| s.viz.clone());
        Ok(canned.unwrap_or_else(|| super::fixtures_data::viz(include_entity)))
    }
    fn web_snapshot(&self, _: &str, _: i64) -> Result<Vec<u8>, BridgeFailure> {
        Ok(JPEG.to_vec())
    }
    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        let delay = self.with(|s| s.send_delay_ms);
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        self.with(|s| s.sent.push((session.id.clone(), text.to_string())));
        Ok(())
    }
    fn send_now(&self, session: &Session) -> Result<(), BridgeFailure> {
        self.with(|s| s.interrupted.push(session.id.clone()));
        Ok(())
    }
    fn pending_prompt(&self, request_id: &str) -> Option<PendingPrompt> {
        self.with(|s| s.prompts.get(request_id).cloned())
    }
    fn respond(
        &self,
        prompt: &PendingPrompt,
        response: &PromptResponse,
    ) -> Result<(), BridgeFailure> {
        self.with(|s| s.responses.push((prompt.id.clone(), response.clone())));
        Ok(())
    }
    fn new_session(&self, cwd: &Path, model: Option<&str>) -> Result<(), BridgeFailure> {
        self.with(|s| {
            s.new_sessions
                .push((cwd.to_path_buf(), model.map(String::from)))
        });
        Ok(())
    }
    fn take_over(&self, session: &Session, stop: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        self.with(|s| {
            s.takeovers.push((session.id.clone(), stop));
            s.takeover.clone().unwrap_or(Ok(TakeoverOutcome {
                session_id: "pty:7C3A91E2".into(),
                stopped_pid: stop.then_some(4242),
            }))
        })
    }
    fn recent_projects(&self) -> Vec<String> {
        vec![
            "/Users/example/src/app".into(),
            "/Users/example/src/notes".into(),
        ]
    }
    fn request_approval(&self, _: &str, _: &str) -> bool {
        self.with(|s| {
            s.approval_calls += 1;
            !s.refuse_approval
        })
    }
    fn start_phone(
        &self,
        _: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure> {
        self.with(|s| match s.phone_error.clone() {
            Some(error) => Err(error),
            None => {
                s.phone_starts.push(options.clone());
                Ok(())
            }
        })
    }
    fn stop_phone(&self, _: &Arc<Connection>) {}
}

/// Devices in memory; tokens are `tok-<n>`.
#[derive(Default)]
pub struct MemoryDevices {
    pub list: Mutex<Vec<(PairedDevice, String)>>,
}

impl MemoryDevices {
    pub fn names(&self) -> Vec<String> {
        lock(&self.list).iter().map(|d| d.0.name.clone()).collect()
    }
    pub fn revoke(&self, id: &str) {
        lock(&self.list).retain(|d| d.0.id != id);
    }
}

impl Devices for MemoryDevices {
    fn authenticate(&self, token: &str) -> Option<PairedDevice> {
        lock(&self.list)
            .iter()
            .find(|d| d.1 == token)
            .map(|d| d.0.clone())
    }
    fn touch(&self, _: &str) {}
    fn register(&self, name: &str, _: &str) -> Result<(PairedDevice, String), String> {
        let mut list = lock(&self.list);
        let n = list.len() + 1;
        let device = PairedDevice {
            id: format!("DEVICE-{n}"),
            name: name.to_string(),
        };
        let token = format!("tok-{n}-{name}");
        list.push((device.clone(), token.clone()));
        Ok((device, token))
    }
}

/// One live code at a time, valid five minutes, used once (Swift `PairingCodes`).
#[derive(Default)]
pub struct MemoryCodes {
    current: Mutex<Option<(String, f64)>>,
}

impl MemoryCodes {
    pub fn issue(&self, now_ms: f64) -> String {
        let code = "K7M2Q9XA".to_string();
        *lock(&self.current) = Some((code.clone(), now_ms + 300_000.0));
        code
    }
}

impl PairingCodes for MemoryCodes {
    fn consume(&self, code: &str, now_ms: f64) -> bool {
        let mut current = lock(&self.current);
        match current.as_ref() {
            Some((live, expires)) if live == code && now_ms < *expires => {
                *current = None;
                true
            }
            _ => false,
        }
    }
}

/// 1, 2, 4, 8 seconds of backoff, then five minutes locked (Swift `AuthRateLimiter`).
#[derive(Default)]
pub struct MemoryLimiter {
    entries: Mutex<HashMap<String, (u32, f64)>>,
}

impl RateLimiter for MemoryLimiter {
    fn is_blocked(&self, address: &str, now_ms: f64) -> bool {
        lock(&self.entries)
            .get(address)
            .is_some_and(|e| e.1 > now_ms)
    }
    fn record_failure(&self, address: &str, now_ms: f64) {
        let mut entries = lock(&self.entries);
        let entry = entries.entry(address.to_string()).or_insert((0, 0.0));
        if entry.0 >= 5 && entry.1 <= now_ms {
            *entry = (0, 0.0);
        }
        entry.0 += 1;
        let delay = if entry.0 >= 5 {
            300_000.0
        } else {
            1000.0 * 2f64.powi(entry.0 as i32 - 1)
        };
        entry.1 = now_ms + delay;
    }
    fn record_success(&self, address: &str) {
        lock(&self.entries).remove(address);
    }
}

#[derive(Default)]
pub struct MemoryAudit {
    pub entries: Mutex<Vec<AuditEntry>>,
}

impl MemoryAudit {
    /// Newest first, as `AuditLog.recent()`.
    pub fn recent(&self) -> Vec<AuditEntry> {
        lock(&self.entries).iter().rev().cloned().collect()
    }
}

impl Audit for MemoryAudit {
    fn append(&self, entry: AuditEntry) {
        lock(&self.entries).push(entry);
    }
}

/// A router on a temp home with the fakes, and a clock the test moves.
pub struct Rig {
    pub hub: Arc<Hub>,
    pub router: Router,
    pub backend: Arc<FakeBackend>,
    pub devices: Arc<MemoryDevices>,
    pub codes: Arc<MemoryCodes>,
    pub audit: Arc<MemoryAudit>,
    pub clock: Arc<Mutex<f64>>,
    pub dir: TempDir,
}

impl Rig {
    pub fn new() -> Rig {
        let dir = TempDir::new("bridge");
        std::fs::create_dir_all(dir.path("home")).expect("home");
        let backend = Arc::new(FakeBackend::default());
        let devices = Arc::new(MemoryDevices::default());
        let codes = Arc::new(MemoryCodes::default());
        let audit = Arc::new(MemoryAudit::default());
        let hub = Arc::new(Hub::new());
        let clock = Arc::new(Mutex::new(NOW_MS));
        let ticking = Arc::clone(&clock);
        let router = Router::new(RouterParts {
            backend: backend.clone(),
            devices: devices.clone(),
            codes: codes.clone(),
            limiter: Arc::new(MemoryLimiter::default()),
            audit: audit.clone(),
            hub: Arc::clone(&hub),
            home: dir.path("home"),
            images_dir: dir.path("images"),
            now_ms: Arc::new(move || *lock(&ticking)),
        });
        Rig {
            hub,
            router,
            backend,
            devices,
            codes,
            audit,
            clock,
            dir,
        }
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path("home")
    }

    pub fn set_clock(&self, ms: f64) {
        *lock(&self.clock) = ms;
    }

    pub fn connect(&self, address: &str) -> (Arc<Connection>, Arc<RecordingIo>) {
        let io = Arc::new(RecordingIo::default());
        let connection = self.hub.connect(address, false, io.clone());
        (connection, io)
    }

    pub fn send(&self, connection: &Arc<Connection>, text: &str) {
        self.router.handle(connection, text);
    }

    /// A paired device's connection, already past `hello`.
    pub fn login(&self, name: &str) -> (Arc<Connection>, Arc<RecordingIo>, String) {
        let (_, token) = self.devices.register(name, "ios").expect("register");
        let (connection, io) = self.connect("192.168.1.20");
        self.send(
            &connection,
            &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
        );
        assert!(connection.is_authenticated());
        (connection, io, token)
    }
}

pub fn session(id: &str, control: Control, state: wire::SessionState) -> Session {
    let mut s = Session::new(id, &format!("Session {id}"), "/p", control, state);
    s.status_updated_ms = NOW_MS;
    s.started_ms = NOW_MS;
    s.pid = Some(1);
    s
}

pub fn message(n: usize, text: &str, role: wire::ChatRole) -> ChatMessage {
    ChatMessage {
        id: format!("M-{n:04}"),
        role,
        text: text.into(),
        time: NOW_MS,
        tool_summary: None,
        queued: false,
        images: Vec::new(),
        kind: None,
        steps: None,
        output: None,
    }
}

pub fn permission(id: &str, session_id: &str) -> PendingPrompt {
    PendingPrompt {
        id: id.into(),
        session_id: session_id.into(),
        kind: PromptKind::Permission,
        tool_name: "Bash".into(),
        summary: "ls".into(),
        created_ms: NOW_MS,
    }
}
