//! The router's security parts ([`crate::bridge::backend`]'s `Devices`, `PairingCodes`,
//! `RateLimiter` and `Audit`) on the transport's stores, which keep the Swift app's files.

use super::audit::{AuditEntry, AuditLog};
use super::devices::DeviceStore;
use super::jwt::Clock;
use super::limiter::{AuthRateLimiter, Verdict};
use super::pairing::PairingCodes;
use crate::bridge::backend;
use std::sync::Arc;

/// `devices.json` for the router. A failed write is logged; the phone is told by the router.
pub struct StoreDevices {
    pub store: Arc<DeviceStore>,
    pub now: Clock,
}

impl backend::Devices for StoreDevices {
    fn authenticate(&self, token: &str) -> Option<backend::PairedDevice> {
        self.store
            .authenticate(token)
            .map(|d| backend::PairedDevice {
                id: d.id,
                name: d.name,
            })
    }

    fn touch(&self, id: &str) {
        if let Err(e) = self.store.touch(id, (self.now)()) {
            eprintln!("notchling bridge: devices.json: {e}");
        }
    }

    fn register(
        &self,
        name: &str,
        platform: &str,
    ) -> Result<(backend::PairedDevice, String), String> {
        let (device, token) = self
            .store
            .register(name, platform, (self.now)())
            .map_err(|e| {
                eprintln!("notchling bridge: pairing {name}: {e}");
                e.to_string()
            })?;
        Ok((
            backend::PairedDevice {
                id: device.id,
                name: device.name,
            },
            token,
        ))
    }
}

impl backend::PairingCodes for PairingCodes {
    fn consume(&self, code: &str, now_ms: f64) -> bool {
        PairingCodes::consume(self, code, now_ms as i64)
    }
}

impl backend::RateLimiter for AuthRateLimiter {
    fn is_blocked(&self, address: &str, now_ms: f64) -> bool {
        matches!(self.check(address, now_ms as i64), Verdict::Blocked(_))
    }

    fn record_failure(&self, address: &str, now_ms: f64) {
        AuthRateLimiter::record_failure(self, address, now_ms as i64);
    }

    fn record_success(&self, address: &str) {
        AuthRateLimiter::record_success(self, address);
    }
}

/// `remote-log.jsonl` for the router, stamped with the bridge's clock.
pub struct LogAudit {
    pub log: Arc<AuditLog>,
    pub now: Clock,
}

impl backend::Audit for LogAudit {
    fn append(&self, entry: backend::AuditEntry) {
        let line = AuditEntry {
            time: (self.now)(),
            device: entry.device,
            op: entry.op,
            session: entry.session,
            length: entry.length.map(|n| n as i64),
            address: entry.address,
            stopped_pid: entry.stopped_pid.map(i64::from),
        };
        if let Err(e) = self.log.append(&line) {
            eprintln!("notchling bridge: audit log: {e}");
        }
    }
}
