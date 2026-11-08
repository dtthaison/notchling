//! `BridgeCore` for the transport: the bridge folder's stores wired together, the live phones, and
//! the door every text frame passes before it reaches the router. Nothing is served before `hello`
//! (a paired token) or `pair` (the one-time code, then the person at the computer allows it).

use super::access::{AccessConfigStore, TunnelGate};
use super::audit::{AuditEntry, AuditLog};
use super::connection::Connection;
use super::devices::{DeviceStore, PairedDevice, StoreError};
use super::jwt::{AccessJwtVerifier, AccessKeySet, Clock, Fetch};
use super::limiter::{AuthRateLimiter, Verdict};
use super::pairing::PairingCodes;
use super::{BridgeHandler, PairApprover, WIRE_VERSION};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The bridge's error codes on the wire.
pub const UNAUTHORIZED: &str = "unauthorized";
pub const INVALID: &str = "invalid";
pub const INTERNAL: &str = "internal";

/// `{"id","ok":true,"data"}`, keys sorted as the Swift bridge writes them.
pub fn ok_reply(id: Option<&Value>, data: Value) -> String {
    let mut object = Map::new();
    object.insert("ok".into(), Value::Bool(true));
    object.insert("data".into(), data);
    if let Some(id) = id {
        object.insert("id".into(), id.clone());
    }
    Value::Object(object).to_string()
}

/// `{"id","ok":false,"error":{"code","message"}}`.
pub fn error_reply(id: Option<&Value>, code: &str, message: &str) -> String {
    let mut object = Map::new();
    object.insert("ok".into(), Value::Bool(false));
    object.insert("error".into(), json!({"code": code, "message": message}));
    if let Some(id) = id {
        object.insert("id".into(), id.clone());
    }
    Value::Object(object).to_string()
}

pub struct Core {
    pub devices: DeviceStore,
    pub codes: PairingCodes,
    pub limiter: Arc<AuthRateLimiter>,
    pub audit: AuditLog,
    pub access: Arc<AccessConfigStore>,
    pub gate: TunnelGate,
    handler: Arc<dyn BridgeHandler>,
    approver: Arc<dyn PairApprover>,
    now: Clock,
    live: Mutex<HashMap<u64, Arc<Connection>>>,
    last_tunnel: Mutex<Option<i64>>,
}

impl Core {
    /// Over the bridge folder (`AgentConsole/bridge`). Reads its files; writes only when a phone
    /// pairs, is seen or is revoked, or an op is audited.
    pub fn new(
        bridge_dir: &Path,
        handler: Arc<dyn BridgeHandler>,
        approver: Arc<dyn PairApprover>,
        fetch: Fetch,
        now: Clock,
    ) -> Core {
        let limiter = Arc::new(AuthRateLimiter::new());
        let access = Arc::new(AccessConfigStore::open(
            bridge_dir.join(super::access::FILE_NAME),
        ));
        let keys = Arc::new(AccessKeySet::new(fetch, now.clone()));
        let config = access.clone();
        let gate = TunnelGate::new(
            Box::new(move || config.config()),
            AccessJwtVerifier::new(keys, now.clone()),
            limiter.clone(),
            now.clone(),
        );
        Core {
            devices: DeviceStore::open(bridge_dir.join(super::devices::FILE_NAME)),
            codes: PairingCodes::new(),
            limiter,
            audit: AuditLog::new(bridge_dir.join(super::audit::FILE_NAME)),
            access,
            gate,
            handler,
            approver,
            now,
            live: Mutex::new(HashMap::new()),
            last_tunnel: Mutex::new(None),
        }
    }

    pub fn now(&self) -> i64 {
        (self.now)()
    }

    pub fn add(&self, connection: Arc<Connection>) {
        if connection.is_tunnel() {
            *lock(&self.last_tunnel) = Some(self.now());
        }
        lock(&self.live).insert(connection.id(), connection);
    }

    /// The connection is gone: the router hears of it when it had said hello.
    pub fn remove(&self, connection: &Arc<Connection>) {
        let was = lock(&self.live).remove(&connection.id());
        if was.is_some() && connection.is_authenticated() {
            self.handler.closed(connection);
        }
    }

    pub fn connections(&self) -> Vec<Arc<Connection>> {
        lock(&self.live).values().cloned().collect()
    }

    /// When a client last got in through the tunnel (`None`: not since the bridge started).
    pub fn last_tunnel_connection(&self) -> Option<i64> {
        *lock(&self.last_tunnel)
    }

    /// Closes every live connection of a device; how many.
    pub fn disconnect(&self, device_id: &str) -> usize {
        let doomed: Vec<Arc<Connection>> = self
            .connections()
            .into_iter()
            .filter(|c| c.device().is_some_and(|d| d.id == device_id))
            .collect();
        for connection in &doomed {
            connection.close();
        }
        doomed.len()
    }

    /// Removes the device and drops its live connections. Its token stops working at once.
    pub fn revoke(&self, device_id: &str) -> Result<Option<PairedDevice>, StoreError> {
        let Some(removed) = self.devices.revoke(device_id)? else {
            return Ok(None);
        };
        self.log(AuditEntry::new(self.now(), &removed.name, "revoke"));
        self.disconnect(device_id);
        Ok(Some(removed))
    }

    pub fn log(&self, entry: AuditEntry) {
        if let Err(e) = self.audit.append(&entry) {
            eprintln!("notchling bridge: audit log: {e}");
        }
    }

    /// One text frame from `connection`: the door before `hello`, the router after.
    pub fn handle(&self, connection: &Arc<Connection>, text: &str) {
        if connection.is_authenticated() {
            self.handler.message(connection, text);
            return;
        }
        if let Verdict::Blocked(_) = self.limiter.check(connection.remote_address(), self.now()) {
            connection.close();
            return;
        }
        let parsed: Option<Map<String, Value>> = serde_json::from_str(text).ok();
        let Some((object, op)) = parsed.and_then(|o| {
            let op = o.get("op").and_then(Value::as_str)?.to_string();
            Some((o, op))
        }) else {
            connection.send_text(&error_reply(
                None,
                INVALID,
                "Tin nhắn không phải JSON hợp lệ.",
            ));
            self.fail(connection);
            return;
        };
        let id = object.get("id");
        match op.as_str() {
            "hello" => self.hello(&object, id, connection),
            "pair" => self.pair(&object, id, connection),
            _ => {
                connection.send_text(&error_reply(
                    id,
                    UNAUTHORIZED,
                    "Cần đăng nhập (hello) trước.",
                ));
                self.fail(connection);
            }
        }
    }

    fn fail(&self, connection: &Connection) {
        self.limiter
            .record_failure(connection.remote_address(), self.now());
        connection.close();
    }

    fn hello(&self, object: &Map<String, Value>, id: Option<&Value>, connection: &Arc<Connection>) {
        let device = object
            .get("token")
            .and_then(Value::as_str)
            .and_then(|t| self.devices.authenticate(t));
        let Some(device) = device else {
            connection.send_text(&error_reply(
                id,
                UNAUTHORIZED,
                "Token không đúng hoặc đã bị thu hồi.",
            ));
            self.fail(connection);
            return;
        };
        self.limiter.record_success(connection.remote_address());
        connection.authenticate(device.clone());
        if let Err(e) = self.devices.touch(&device.id, self.now()) {
            eprintln!("notchling bridge: devices.json: {e}");
        }
        let data =
            json!({"device": {"id": device.id, "name": device.name}, "version": WIRE_VERSION});
        connection.send_text(&ok_reply(id, data));
        self.handler.opened(connection);
    }

    fn pair(&self, object: &Map<String, Value>, id: Option<&Value>, connection: &Arc<Connection>) {
        let code = object.get("code").and_then(Value::as_str);
        if !code.is_some_and(|c| self.codes.consume(c, self.now())) {
            connection.send_text(&error_reply(
                id,
                UNAUTHORIZED,
                "Mã ghép đôi sai hoặc đã hết hạn.",
            ));
            self.fail(connection);
            return;
        }
        let info = object.get("device");
        let field = |k: &str| {
            info.and_then(|i| i.get(k))
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
        };
        let name = field("name").unwrap_or_default();
        let platform = field("platform").unwrap_or_else(|| "unknown".into());
        if name.is_empty() {
            connection.send_text(&error_reply(id, INVALID, "Thiếu tên thiết bị."));
            self.fail(connection);
            return;
        }
        if !self.approver.approve(&name, &platform) {
            connection.send_text(&error_reply(
                id,
                UNAUTHORIZED,
                "Mac đã từ chối thiết bị này.",
            ));
            connection.close();
            return;
        }
        match self.devices.register(&name, &platform, self.now()) {
            Ok((device, token)) => {
                self.limiter.record_success(connection.remote_address());
                connection.authenticate(device.clone());
                let mut entry = AuditEntry::new(self.now(), &device.name, "pair");
                entry.address = connection.audit_address().map(str::to_string);
                self.log(entry);
                let data = json!({"token": token, "deviceId": device.id, "version": WIRE_VERSION});
                connection.send_text(&ok_reply(id, data));
                self.handler.opened(connection);
            }
            Err(e) => {
                eprintln!("notchling bridge: pairing {name}: {e}");
                connection.send_text(&error_reply(id, INTERNAL, "Không lưu được thiết bị."));
                connection.close();
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "core_tests.rs"]
pub(crate) mod tests;
