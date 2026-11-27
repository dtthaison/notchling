use crate::acting_ops::{ActingOps, Room};
use crate::bridge_backend::{Approvals, DaemonActing, DaemonSources};
use crate::bridge_cluster::PhoneCluster;
use crate::every_session::EverySession;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use notchling_engine::bridge::events_audit::EventsAudit;
use notchling_engine::bridge::live::LiveBackend;
use notchling_engine::bridge::net::devices::Replica;
use notchling_engine::bridge::net::{fetch, files, Bridge, BridgeConfig};
use notchling_events::kinds::choices::SwitchName;
use notchling_events::kinds::security::SwitchChanged;
use notchling_events::spec::Outcome;
use notchling_events::{scope, Event};
use notchling_local_api::{ops, topics, ApiError, OpSpec};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
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
    cluster: Arc<PhoneCluster>,
    lan_port: u16,
    served: Mutex<crate::cluster_relay::Served>,
    replica: Arc<OnceLock<Replica>>,
    events: Option<Events>,
}

impl BridgeOps {
    pub fn new(
        setup: BridgeSetup,
        sessions: Arc<Mutex<Value>>,
        acting: Arc<ActingOps>,
        room: Arc<dyn Room>,
        inbox: Arc<InboxOps>,
        every: Arc<EverySession>,
    ) -> BridgeOps {
        let approvals = Arc::new(Approvals::new(room.clone(), setup.approval_wait));
        let cluster = PhoneCluster::none();
        let backend = LiveBackend::new(
            DaemonSources {
                sessions: sessions.clone(),
                acting: acting.clone(),
                approvals: approvals.clone(),
                inbox,
                every,
                cluster: cluster.clone(),
                projects_dir: setup.projects_dir.clone(),
            },
            DaemonActing {
                ops: acting,
                sessions,
                cluster: cluster.clone(),
            },
        );
        let mut config = BridgeConfig::new(setup.data_dir, setup.home, setup.images_dir)
            .with_ports(setup.lan_port, setup.tunnel_port);
        if setup.loopback_only {
            config
                .lan
                .set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        }
        let replica: Arc<OnceLock<Replica>> = Arc::new(OnceLock::new());
        let source = replica.clone();
        let from_set: Replica = Arc::new(move || source.get().and_then(|find| find()));
        BridgeOps {
            bridge: Bridge::new(
                config,
                Arc::new(backend),
                fetch::live(),
                Arc::new(files::now_ms),
            )
            .with_replica(from_set),
            replica,
            approvals,
            room,
            cluster,
            lan_port: setup.lan_port,
            served: Mutex::new(crate::cluster_relay::Served {
                port: setup.lan_port,
                fingerprint: None,
            }),
            events: None,
        }
    }

    pub fn cluster(&self) -> Arc<PhoneCluster> {
        self.cluster.clone()
    }

    pub fn served(&self) -> crate::cluster_relay::Served {
        self.served
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn remember(&self, status: &Value) {
        let port = status["port"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or(self.lan_port);
        let fingerprint = status["fingerprint"].as_str().map(str::to_string);
        *self.served.lock().unwrap_or_else(|e| e.into_inner()) =
            crate::cluster_relay::Served { port, fingerprint };
    }

    pub fn with_node_identity(mut self, dir: PathBuf) -> BridgeOps {
        self.bridge = self.bridge.with_node_identity(dir);
        self
    }

    pub fn set_replica(&self, find: Replica) {
        let _ = self.replica.set(find);
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
                self.remember(&status);
                if let Some(events) = &self.events {
                    let cause = crate::switches::cause_of(args);
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
        let status = self.bridge.set(false);
        self.remember(&status);
    }

    pub fn release(&self) {
        self.cluster().detach();
    }
}

#[cfg(test)]
#[path = "bridge_ops_tests.rs"]
mod tests;
