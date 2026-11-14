use crate::acting_ops::Room;
use crate::cluster_node::{ClusterLog, LocalCalls};
use notchling_engine::cluster::policy::cluster_may_set;
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::Desired;
use notchling_events::kinds::security::SwitchCause;
use notchling_events::spec::Outcome;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Handler, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub const NODE_SWITCH_SET: &str = "node.switch.set";

pub trait SwitchPanel: Send + Sync {
    fn get(&self, switch: &str) -> Option<bool>;
    fn set(&self, switch: &str, on: bool) -> Result<(), ApiError>;
}

#[derive(Default)]
pub struct LateHandler(OnceLock<Weak<dyn Handler>>);

impl LateHandler {
    pub fn serve(&self, handler: &Arc<dyn Handler>) {
        let _ = self.0.set(Arc::downgrade(handler));
    }

    pub fn get(&self) -> Result<Arc<dyn Handler>, ApiError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the daemon isn't serving yet"))
    }
}

fn the_app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    }
}

pub struct OpsPanel {
    handler: Arc<LateHandler>,
}

impl OpsPanel {
    pub fn new(handler: Arc<LateHandler>) -> OpsPanel {
        OpsPanel { handler }
    }

    fn ask(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.handler.get()?.call(&the_app(), op, &args)
    }
}

impl SwitchPanel for OpsPanel {
    fn get(&self, switch: &str) -> Option<bool> {
        match switch {
            "acting" => self.ask(ops::SETTINGS_GET_ACTING, json!({})).ok()?["acting"].as_bool(),
            "bridge" => {
                let status = self.ask(ops::BRIDGE_STATUS, json!({})).ok()?;
                status["state"].as_str().map(|s| s != "off")
            }
            other => self.ask(ops::SETTINGS_SWITCHES, json!({})).ok()?[other].as_bool(),
        }
    }

    fn set(&self, switch: &str, on: bool) -> Result<(), ApiError> {
        let (op, args) = match switch {
            "acting" => (ops::SETTINGS_SET_ACTING, json!({"on": on})),
            "bridge" => (ops::BRIDGE_SET, json!({"on": on})),
            other => (ops::SETTINGS_SET_SWITCH, json!({"switch": other, "on": on})),
        };
        self.ask(op, args).map(|_| ())
    }
}

pub struct HeadlessSwitches {
    panel: Arc<dyn SwitchPanel>,
    room: Arc<dyn Room>,
    audit: ClusterLog,
    gate: Mutex<()>,
}

fn forbidden(text: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, text)
}

impl HeadlessSwitches {
    pub fn new(
        panel: Arc<dyn SwitchPanel>,
        room: Arc<dyn Room>,
        audit: ClusterLog,
    ) -> HeadlessSwitches {
        HeadlessSwitches {
            panel,
            room,
            audit,
            gate: Mutex::new(()),
        }
    }

    pub fn follow(&self, desired: Desired) {
        for (switch, on) in [
            ("acting", desired.acting),
            ("bridge", desired.bridge),
            ("install", desired.install),
        ] {
            let _ = self.set(switch, on, None);
        }
    }

    pub fn set(&self, switch: &str, on: bool, _from: Option<&str>) -> Result<Value, ApiError> {
        if !cluster_may_set(switch) {
            return Err(forbidden(
                "the cluster sets only acting, bridge and install",
            ));
        }
        if self.room.can_answer() {
            return Ok(json!({"switch": switch, "on": on, "applied": false}));
        }
        let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if self.panel.get(switch) == Some(on) {
            return Ok(json!({"switch": switch, "on": on, "applied": false}));
        }
        let result = self.panel.set(switch, on);
        let outcome = if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Failed
        };
        self.audit
            .switch(switch, on, SwitchCause::ClusterNode, outcome);
        result.map(|_| json!({"switch": switch, "on": on, "applied": true}))
    }
}

pub struct NodeCalls {
    pub inner: Arc<dyn LocalCalls>,
    pub switches: Arc<HeadlessSwitches>,
}

impl LocalCalls for NodeCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        if op != NODE_SWITCH_SET {
            return self.inner.call(op, args, origin);
        }
        let usage =
            "node.switch.set takes {\"switch\": \"acting\"|\"bridge\"|\"install\", \"on\": bool}";
        let switch = args.get("switch").and_then(Value::as_str);
        let on = args.get("on").and_then(Value::as_bool);
        let (Some(switch), Some(on)) = (switch, on) else {
            return Err(ApiError::new(ErrorCode::BadRequest, usage));
        };
        self.switches.set(switch, on, Some(origin.node.as_str()))
    }
}

#[cfg(test)]
#[path = "cluster_switches_tests.rs"]
mod tests;
