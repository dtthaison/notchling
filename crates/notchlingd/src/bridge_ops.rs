//! Milestone 4's phone bridge in the daemon: the app-tier `bridge.*` ops over the engine's
//! [`Bridge`] (E5, TLS on the LAN port, the tunnel origin on loopback, the router).
//!
//! The bridge is off at every start, whatever a file says: it binds no port and writes nothing
//! under the data folder (`AgentConsole`) until the app's `bridge.set {"on": true}` in this run
//! (decision 0007's note: files at rest are not a tier boundary). While the Swift app still runs
//! its own bridge it holds both ports, so turning this one on then fails with a busy port and says
//! so (decision 0011: one owner).

use crate::acting_ops::{ActingOps, Room};
use crate::bridge_backend::{Approvals, DaemonActing, DaemonSources};
use notchling_engine::bridge::live::LiveBackend;
use notchling_engine::bridge::net::{fetch, files, Bridge, BridgeConfig};
use notchling_local_api::{ops, topics, ApiError, OpSpec};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Every bridge op; all app tier.
pub const BRIDGE_OPS: [&str; 7] = [
    ops::BRIDGE_SET,
    ops::BRIDGE_STATUS,
    ops::BRIDGE_PAIR_BEGIN,
    ops::BRIDGE_PAIR_ANSWER,
    ops::BRIDGE_DEVICES,
    ops::BRIDGE_REVOKE,
    ops::BRIDGE_TUNNEL_STATUS,
];

/// Where the bridge lives and listens. `data_dir` is `AgentConsole`, the Swift app's folder, so
/// paired phones and the Cloudflare setup carry over.
#[derive(Clone, Debug)]
pub struct BridgeSetup {
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub images_dir: PathBuf,
    /// `~/.claude/projects`, for the phone's recent folders.
    pub projects_dir: PathBuf,
    /// The LAN and tunnel ports (47821 and 47822 unless a test or a flag says otherwise).
    pub lan_port: u16,
    pub tunnel_port: u16,
    /// Binds the LAN port on loopback only (tests).
    pub loopback_only: bool,
    pub approval_wait: Duration,
}

pub struct BridgeOps {
    bridge: Bridge,
    approvals: Arc<Approvals>,
    room: Arc<dyn Room>,
}

impl BridgeOps {
    /// Off.
    pub fn new(
        setup: BridgeSetup,
        sessions: Arc<Mutex<Value>>,
        acting: Arc<ActingOps>,
        room: Arc<dyn Room>,
    ) -> BridgeOps {
        let approvals = Arc::new(Approvals::new(room.clone(), setup.approval_wait));
        let backend = LiveBackend::new(
            DaemonSources {
                sessions: sessions.clone(),
                acting: acting.clone(),
                approvals: approvals.clone(),
                projects_dir: setup.projects_dir.clone(),
            },
            DaemonActing {
                ops: acting,
                sessions,
            },
        );
        let mut config = BridgeConfig::new(setup.data_dir, setup.home, setup.images_dir)
            .with_ports(setup.lan_port, setup.tunnel_port);
        if setup.loopback_only {
            config
                .lan
                .set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        }
        BridgeOps {
            bridge: Bridge::new(
                config,
                Arc::new(backend),
                fetch::live(),
                Arc::new(files::now_ms),
            ),
            approvals,
            room,
        }
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        BRIDGE_OPS.contains(&op).then_some(OpSpec::APP)
    }

    pub fn is_on(&self) -> bool {
        self.bridge.is_on()
    }

    /// The bound (LAN, tunnel) ports while on.
    pub fn ports(&self) -> Option<(u16, u16)> {
        self.bridge.ports()
    }

    pub fn call(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::BRIDGE_STATUS => Ok(self.bridge.status()),
            ops::BRIDGE_SET => {
                let on = args.get("on").and_then(Value::as_bool).ok_or_else(|| {
                    ApiError::bad_request("bridge.set takes {\"on\": true|false}")
                })?;
                if !on {
                    // Halted first: a pair approved just before the refusal can't write either.
                    self.bridge.halt();
                    self.approvals.refuse_all();
                }
                let status = self.bridge.set(on);
                self.room
                    .publish(topics::BRIDGE, topics::BRIDGE_CHANGED, &status);
                Ok(status)
            }
            ops::BRIDGE_PAIR_BEGIN => self.bridge.pair_begin().map_err(ApiError::bad_request),
            ops::BRIDGE_PAIR_ANSWER => {
                let id = args.get("request").and_then(Value::as_str);
                let allow = args.get("allow").and_then(Value::as_bool);
                let (Some(id), Some(allow)) = (id, allow) else {
                    return Err(ApiError::bad_request(
                        "bridge.pair.answer takes {\"request\": id, \"allow\": bool}",
                    ));
                };
                Ok(json!({"answered": self.approvals.answer(id, allow)}))
            }
            ops::BRIDGE_DEVICES => self.bridge.devices().map_err(ApiError::internal),
            ops::BRIDGE_REVOKE => {
                let device = args
                    .get("device")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::bad_request("bridge.revoke takes {\"device\": id}"))?;
                self.bridge.revoke(device).map_err(ApiError::bad_request)
            }
            ops::BRIDGE_TUNNEL_STATUS => Ok(self.bridge.tunnel_status()),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    /// The daemon is stopping: phones waiting to pair are refused and the ports close.
    pub fn close(&self) {
        self.approvals.refuse_all();
        self.bridge.set(false);
    }
}

#[cfg(test)]
#[path = "bridge_ops_tests.rs"]
mod tests;
