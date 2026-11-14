use crate::acting_ops::{ActingOps, Room};
use crate::bridge_backend::{Approvals, DaemonActing, DaemonSources};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use notchling_engine::bridge::events_audit::EventsAudit;
use notchling_engine::bridge::live::LiveBackend;
use notchling_engine::bridge::net::{fetch, files, Bridge, BridgeConfig};
use notchling_events::kinds::choices::SwitchName;
use notchling_events::kinds::security::{SwitchCause, SwitchChanged};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event};
use notchling_local_api::{ops, topics, ApiError, OpSpec};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const BRIDGE_OPS: [&str; 7] = [
    ops::BRIDGE_SET,
    ops::BRIDGE_STATUS,
    ops::BRIDGE_PAIR_BEGIN,
    ops::BRIDGE_PAIR_ANSWER,
    ops::BRIDGE_DEVICES,
    ops::BRIDGE_REVOKE,
    ops::BRIDGE_TUNNEL_STATUS,
];

#[derive(Clone, Debug)]
pub struct BridgeSetup {
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub images_dir: PathBuf,
    pub projects_dir: PathBuf,
    pub lan_port: u16,
    pub tunnel_port: u16,
    pub loopback_only: bool,
    pub approval_wait: Duration,
}

pub struct BridgeOps {
    bridge: Bridge,
    approvals: Arc<Approvals>,
    room: Arc<dyn Room>,
    events: Option<Events>,
}

impl BridgeOps {
    pub fn new(
        setup: BridgeSetup,
        sessions: Arc<Mutex<Value>>,
        acting: Arc<ActingOps>,
        room: Arc<dyn Room>,
        inbox: Arc<InboxOps>,
    ) -> BridgeOps {
        let approvals = Arc::new(Approvals::new(room.clone(), setup.approval_wait));
        let backend = LiveBackend::new(
            DaemonSources {
                sessions: sessions.clone(),
                acting: acting.clone(),
                approvals: approvals.clone(),
                inbox,
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
            events: None,
        }
    }

    pub fn with_events(mut self, events: Events) -> BridgeOps {
        let audit = EventsAudit::new(events.emit(), Arc::new(files::now_ms));
        self.bridge = self.bridge.with_audit(Arc::new(audit));
        self.events = Some(events);
        self
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        BRIDGE_OPS.contains(&op).then_some(OpSpec::APP)
    }

    pub fn approvals(&self) -> Arc<Approvals> {
        self.approvals.clone()
    }

    pub fn is_on(&self) -> bool {
        self.bridge.is_on()
    }

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
                    self.bridge.halt();
                    self.approvals.refuse_all();
                }
                let status = self.bridge.set(on);
                if let Some(events) = &self.events {
                    let cause = match args.get("restored").and_then(Value::as_bool) {
                        Some(true) => SwitchCause::RestoredAtLaunch,
                        _ => SwitchCause::User,
                    };
                    events.push(Event::new(
                        &SwitchChanged {
                            switch: SwitchName::Bridge,
                            on,
                            cause,
                        },
                        Outcome::Ok,
                        crate::acting_ops::now_ms(),
                    ));
                    scope::mark_recorded();
                }
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

    pub fn close(&self) {
        self.bridge.halt();
        self.approvals.refuse_all();
        self.bridge.set(false);
    }
}

#[cfg(test)]
#[path = "bridge_ops_tests.rs"]
mod tests;
