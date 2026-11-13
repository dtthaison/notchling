use super::access::{AccessConfigStore, TunnelGate};
use super::audit::{AuditEntry, AuditLog};
use super::devices::{DeviceStore, PairedDevice, StoreError};
use super::jwt::{AccessJwtVerifier, AccessKeySet, Clock, Fetch};
use super::limiter::AuthRateLimiter;
use super::pairing::PairingCodes;
use super::parts::{LogAudit, StoreDevices};
use crate::bridge::{BridgeBackend, Connection, ConnectionIo, Hub, Router, RouterParts};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct CoreParts {
    pub backend: Arc<dyn BridgeBackend>,
    pub home: PathBuf,
    pub images_dir: PathBuf,
    pub fetch: Fetch,
    pub now: Clock,
}

pub struct Core {
    pub devices: Arc<DeviceStore>,
    pub codes: Arc<PairingCodes>,
    pub limiter: Arc<AuthRateLimiter>,
    pub audit: Arc<AuditLog>,
    pub access: Arc<AccessConfigStore>,
    pub gate: TunnelGate,
    pub hub: Arc<Hub>,
    pub router: Router,
    backend: Arc<dyn BridgeBackend>,
    now: Clock,
    last_tunnel: Mutex<Option<i64>>,
}

impl Core {
    pub fn new(bridge_dir: &Path, parts: CoreParts) -> Core {
        let now = parts.now;
        let limiter = Arc::new(AuthRateLimiter::new());
        let access = Arc::new(AccessConfigStore::open(
            bridge_dir.join(super::access::FILE_NAME),
        ));
        let keys = Arc::new(AccessKeySet::new(parts.fetch, now.clone()));
        let config = access.clone();
        let gate = TunnelGate::new(
            Box::new(move || config.config()),
            AccessJwtVerifier::new(keys, now.clone()),
            limiter.clone(),
            now.clone(),
        );
        let devices = Arc::new(DeviceStore::open(
            bridge_dir.join(super::devices::FILE_NAME),
        ));
        let codes = Arc::new(PairingCodes::new());
        let audit = Arc::new(AuditLog::new(bridge_dir.join(super::audit::FILE_NAME)));
        let hub = Arc::new(Hub::new());
        let clock = now.clone();
        let router = Router::new(RouterParts {
            backend: parts.backend.clone(),
            devices: Arc::new(StoreDevices {
                store: devices.clone(),
                now: now.clone(),
            }),
            codes: codes.clone(),
            limiter: limiter.clone(),
            audit: Arc::new(LogAudit {
                log: audit.clone(),
                now: now.clone(),
            }),
            hub: hub.clone(),
            home: parts.home,
            images_dir: parts.images_dir,
            now_ms: Arc::new(move || clock() as f64),
        });
        Core {
            devices,
            codes,
            limiter,
            audit,
            access,
            gate,
            hub,
            router,
            backend: parts.backend,
            now,
            last_tunnel: Mutex::new(None),
        }
    }

    pub fn now(&self) -> i64 {
        (self.now)()
    }

    pub fn connect(
        &self,
        address: &str,
        is_tunnel: bool,
        io: Arc<dyn ConnectionIo>,
    ) -> Arc<Connection> {
        if is_tunnel {
            *lock(&self.last_tunnel) = Some(self.now());
        }
        self.hub.connect(address, is_tunnel, io)
    }

    pub fn handle(&self, connection: &Arc<Connection>, text: &str) {
        self.router.handle(connection, text);
    }

    pub fn closed(&self, connection: &Arc<Connection>) {
        self.router.closed(connection);
    }

    pub fn halt(&self) {
        self.devices.halt();
        self.audit.halt();
    }

    pub fn tick(&self) {
        self.router.sweep_uploads();
        self.hub.tick(self.backend.as_ref());
    }

    pub fn connections(&self) -> usize {
        self.hub.all().len()
    }

    pub fn last_tunnel_connection(&self) -> Option<i64> {
        *lock(&self.last_tunnel)
    }

    pub fn revoke(&self, device_id: &str) -> Result<Option<PairedDevice>, StoreError> {
        let Some(removed) = self.devices.revoke(device_id)? else {
            return Ok(None);
        };
        let entry = AuditEntry::new(self.now(), &removed.name, "revoke");
        if let Err(e) = self.audit.append(&entry) {
            eprintln!("notchling bridge: audit log: {e}");
        }
        self.hub.disconnect(device_id);
        Ok(Some(removed))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "core_tests.rs"]
pub(crate) mod tests;
