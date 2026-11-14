use super::backend::{Audit, AuditEntry, BridgeBackend, Devices, PairingCodes, RateLimiter};
use super::connection::{lock, Connection};
use super::hub::Hub;
use super::uploads::{ImageStore, Uploads};
use super::wire::{self, BridgeFailure};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_TEXT_CHARACTERS: usize = 20_000;
pub const MODELS: [&str; 4] = ["default", "opus", "sonnet", "haiku"];

pub struct RouterParts {
    pub backend: Arc<dyn BridgeBackend>,
    pub devices: Arc<dyn Devices>,
    pub codes: Arc<dyn PairingCodes>,
    pub limiter: Arc<dyn RateLimiter>,
    pub audit: Arc<dyn Audit>,
    pub hub: Arc<Hub>,
    pub home: PathBuf,
    pub images_dir: PathBuf,
    pub now_ms: Arc<dyn Fn() -> f64 + Send + Sync>,
}

pub struct Router {
    pub(crate) backend: Arc<dyn BridgeBackend>,
    devices: Arc<dyn Devices>,
    codes: Arc<dyn PairingCodes>,
    limiter: Arc<dyn RateLimiter>,
    audit: Arc<dyn Audit>,
    pub(crate) hub: Arc<Hub>,
    pub(crate) home: PathBuf,
    pub(crate) images: ImageStore,
    pub(crate) uploads: Uploads,
    send_now_gate: Mutex<HashMap<String, f64>>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

const SEND_NOW_INTERVAL_MS: f64 = 3000.0;

impl Router {
    pub fn new(parts: RouterParts) -> Router {
        Router {
            backend: parts.backend,
            devices: parts.devices,
            codes: parts.codes,
            limiter: parts.limiter,
            audit: parts.audit,
            hub: parts.hub,
            home: parts.home,
            images: ImageStore::new(parts.images_dir),
            uploads: Uploads::default(),
            send_now_gate: Mutex::new(HashMap::new()),
            now: parts.now_ms,
        }
    }

    pub(crate) fn now_ms(&self) -> f64 {
        (self.now)()
    }

    pub fn pending_uploads(&self) -> usize {
        self.uploads.pending()
    }

    pub fn sweep_uploads(&self) {
        self.uploads.sweep(self.now_ms());
    }

    pub fn handle(&self, connection: &Arc<Connection>, text: &str) {
        let authenticated = connection.is_authenticated();
        if !authenticated
            && self
                .limiter
                .is_blocked(&connection.remote_address, self.now_ms())
        {
            connection.close();
            return;
        }
        let parsed = (text.len() <= MAX_MESSAGE_BYTES)
            .then(|| serde_json::from_str::<Value>(text).ok())
            .flatten();
        let Some((body, op)) = parsed.and_then(|v| match v {
            Value::Object(map) => {
                let op = map.get("op").and_then(Value::as_str).map(String::from)?;
                Some((map, op))
            }
            _ => None,
        }) else {
            reply_error(
                connection,
                None,
                &BridgeFailure::invalid("The message is not valid JSON."),
            );
            if !authenticated {
                self.fail(connection);
            }
            return;
        };
        let id = body.get("id");

        if !authenticated {
            match op.as_str() {
                "hello" => self.hello(&body, connection),
                "pair" => self.pair(&body, connection),
                _ => {
                    reply_error(
                        connection,
                        id,
                        &BridgeFailure::unauthorized("Sign in (hello) first."),
                    );
                    self.fail(connection);
                }
            }
            return;
        }

        match self.dispatch(&op, &body, connection) {
            Ok(data) => connection.send_text(wire::ok(id, data)),
            Err(failure) => reply_error(connection, id, &failure),
        }
    }

    pub fn closed(&self, connection: &Arc<Connection>) {
        self.uploads.drop_connection(connection.id);
        let streaming = std::mem::take(&mut connection.subscription().phone_active);
        if streaming {
            self.backend.stop_phone(connection);
        }
        self.hub.remove(connection);
    }

    fn fail(&self, connection: &Connection) {
        self.limiter
            .record_failure(&connection.remote_address, self.now_ms());
        connection.close();
    }

    fn hello(&self, body: &Map<String, Value>, connection: &Arc<Connection>) {
        let id = body.get("id");
        let device = body
            .get("token")
            .and_then(Value::as_str)
            .and_then(|token| self.devices.authenticate(token));
        let Some(device) = device else {
            reply_error(
                connection,
                id,
                &BridgeFailure::unauthorized("The token is wrong or was revoked."),
            );
            self.fail(connection);
            return;
        };
        self.limiter.record_success(&connection.remote_address);
        self.devices.touch(&device.id);
        let data =
            json!({"device": {"id": device.id, "name": device.name}, "version": wire::VERSION});
        connection.authenticate(device);
        connection.send_text(wire::ok(id, data));
    }

    fn pair(&self, body: &Map<String, Value>, connection: &Arc<Connection>) {
        let id = body.get("id");
        let code_ok = body
            .get("code")
            .and_then(Value::as_str)
            .is_some_and(|code| self.codes.consume(code, self.now_ms()));
        if !code_ok {
            reply_error(
                connection,
                id,
                &BridgeFailure::unauthorized("The pairing code is wrong or expired."),
            );
            self.fail(connection);
            return;
        }
        let info = body.get("device");
        let field = |key: &str| {
            info.and_then(|d| d.get(key))
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
        };
        let name = field("name").unwrap_or_default();
        let platform = field("platform").unwrap_or_else(|| "unknown".into());
        if name.is_empty() {
            reply_error(
                connection,
                id,
                &BridgeFailure::invalid("The device name is missing."),
            );
            self.fail(connection);
            return;
        }
        if !self
            .backend
            .request_approval(&name, &platform, &connection.remote_address)
        {
            reply_error(
                connection,
                id,
                &BridgeFailure::unauthorized("The Mac refused this device."),
            );
            connection.close();
            return;
        }
        match self.devices.register(&name, &platform) {
            Ok((device, token)) => {
                self.limiter.record_success(&connection.remote_address);
                let data = json!({"token": token, "deviceId": device.id, "version": wire::VERSION});
                self.audit.append(AuditEntry {
                    device: device.name.clone(),
                    op: "pair".into(),
                    address: connection.audit_address(),
                    ..AuditEntry::default()
                });
                connection.authenticate(device);
                connection.send_text(wire::ok(id, data));
            }
            Err(_) => {
                reply_error(
                    connection,
                    id,
                    &BridgeFailure::internal("Can't save the device."),
                );
                connection.close();
            }
        }
    }

    fn dispatch(
        &self,
        op: &str,
        body: &Map<String, Value>,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let id = body.get("id");
        match op {
            "ping" => Ok(json!({"time": self.now_ms() as i64})),
            "sessions.list" => {
                let list: Vec<_> = self
                    .backend
                    .sessions()
                    .iter()
                    .map(|s| s.to_wire())
                    .collect();
                let mut data = json!({"sessions": wire::object(&list)});
                let models = self.backend.models();
                if !models.is_empty() {
                    data["models"] = Value::Array(
                        models
                            .iter()
                            .map(|m| json!({"id": m.id, "name": m.name}))
                            .collect(),
                    );
                }
                Ok(data)
            }
            "chat.get" => self.chat_get(body),
            "viz.get" => self.viz_get(body),
            "web.snapshot" => self.web_snapshot(body, id, connection),
            "session.message" => self.session_message(body, connection),
            "session.sendNow" => self.session_send_now(body, connection),
            "session.image.begin" => self.image_begin(body, connection),
            "session.image.chunk" => self.image_chunk(body, connection),
            "session.image.end" => self.image_end(body, connection),
            "commands.list" => self.commands_list(body),
            "prompt.answer" => self.prompt_answer(body, connection),
            "session.new" => self.session_new(body, connection),
            "chat.image" => self.chat_image(body, id, connection),
            "session.takeover" => self.session_takeover(body, connection),
            "inbox.list" => Ok(self.inbox_list()),
            "inbox.answer" => self.inbox_answer(body, connection),
            "projects.recent" => Ok(json!({"projects": self.backend.recent_projects()})),
            "subscribe" => self.subscribe(body, connection),
            "unsubscribe" => {
                self.unsubscribe(connection);
                Ok(json!({"subscribed": false}))
            }
            _ => Err(BridgeFailure::invalid(format!("No such op \"{op}\"."))),
        }
    }

    pub(crate) fn audit(
        &self,
        connection: &Connection,
        op: &str,
        session: Option<&str>,
        length: Option<usize>,
    ) {
        self.audit.append(AuditEntry {
            device: connection.device_name(),
            op: op.into(),
            session: session.map(String::from),
            length,
            address: connection.audit_address(),
            stopped_pid: None,
        });
    }

    pub(crate) fn audit_entry(&self, entry: AuditEntry) {
        self.audit.append(entry);
    }

    pub(crate) fn allow_send_now(&self, session_id: &str) -> bool {
        let now = self.now_ms();
        let mut last = lock(&self.send_now_gate);
        if last
            .get(session_id)
            .is_some_and(|at| now - at < SEND_NOW_INTERVAL_MS)
        {
            return false;
        }
        last.insert(session_id.to_string(), now);
        true
    }
}

pub(crate) fn reply_error(connection: &Connection, id: Option<&Value>, failure: &BridgeFailure) {
    connection.send_text(wire::error(id, failure));
}

pub fn validated_folder(path: &str, home: &Path) -> Result<PathBuf, BridgeFailure> {
    if !path.starts_with('/') {
        return Err(BridgeFailure::invalid("cwd must be an absolute path."));
    }
    let resolved = resolve(Path::new(path));
    let home = resolve(home);
    if !resolved.starts_with(&home) {
        return Err(BridgeFailure::invalid(
            "Only folders inside the Mac's home folder can be opened.",
        ));
    }
    if !resolved.is_dir() {
        return Err(BridgeFailure::not_found("The folder doesn't exist."));
    }
    Ok(resolved)
}

fn resolve(path: &Path) -> PathBuf {
    let mut plain = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                plain.pop();
            }
            Component::CurDir => {}
            other => plain.push(other.as_os_str()),
        }
    }
    let mut existing = plain.clone();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&existing) {
            return rest
                .iter()
                .rev()
                .fold(real, |acc: PathBuf, part| acc.join(part));
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return plain,
        }
    }
}
