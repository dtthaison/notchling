use crate::acting_ops::Room;
use crate::commands::{command_of, INTERNAL, PLANNED, USER_ONLY};
use crate::daemon_wiring::Wired;
use crate::op_group::OpGroup;
use notchling_local_api::{ops, topics, ApiError, Caller, OpSpec, Tier, API_VERSION};
use serde_json::{json, Value};
use std::sync::Arc;

pub const SWITCH_NAMES: [&str; 6] = [
    "acting", "bridge", "accounts", "vault", "install", "cluster",
];

pub type Switched = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub type Sessioned = Arc<dyn Fn(u32) -> Option<String> + Send + Sync>;

pub struct CapabilitiesOps {
    version: String,
    switched: Switched,
    session_of: Sessioned,
    room: Arc<dyn Room>,
}

pub fn now_for(app: bool, op: &str, switched: &dyn Fn(&str) -> bool) -> Value {
    if let Some(i) = INTERNAL.iter().find(|i| i.op == op) {
        return match (i.why, app) {
            ("app_ui", _) if app => json!({"op": op, "now": "open"}),
            ("app_ui", _) => json!({"op": op, "now": "app_only"}),
            _ => json!({"op": op, "now": "internal"}),
        };
    }
    if app {
        return json!({"op": op, "now": "open"});
    }
    if USER_ONLY.iter().any(|u| u.op == op) {
        return json!({"op": op, "now": "never"});
    }
    if let Some(p) = PLANNED.iter().find(|p| p.op == op) {
        return json!({"op": op, "now": "not_yet", "phase": p.phase});
    }
    let Some((row, used)) = command_of(op) else {
        return json!({"op": op, "now": "internal"});
    };
    if used.tier == "app" {
        return json!({"op": op, "now": "app_only"});
    }
    if let Some(switch) = used.switch.filter(|s| !switched(s)) {
        return json!({"op": op, "now": "needs_switch", "switch": switch});
    }
    if row.consent == "request" {
        return json!({"op": op, "now": "needs_consent"});
    }
    json!({"op": op, "now": "open"})
}

impl CapabilitiesOps {
    pub fn new(
        version: &str,
        switched: Switched,
        session_of: Sessioned,
        room: Arc<dyn Room>,
    ) -> CapabilitiesOps {
        CapabilitiesOps {
            version: version.to_string(),
            switched,
            session_of,
            room,
        }
    }

    pub fn wired(wired: &Wired) -> CapabilitiesOps {
        let acting = &wired.acting;
        let switched: Switched = {
            let (acting, bridge, switches) =
                (acting.clone(), wired.bridge.clone(), wired.switches.clone());
            Arc::new(move |name: &str| match name {
                "acting" => acting.is_on(),
                "bridge" => bridge.is_on(),
                other => switches.get(other).is_some_and(|flag| flag.is_on()),
            })
        };
        let session_of: Sessioned = {
            let acting = acting.clone();
            Arc::new(move |pid| acting.session_of(pid))
        };
        CapabilitiesOps::new(
            env!("CARGO_PKG_VERSION"),
            switched,
            session_of,
            acting.room.clone(),
        )
    }

    fn answer(&self, caller: &Caller) -> Value {
        let app = caller.tier == Tier::App;
        let session = caller
            .pid
            .filter(|_| !app)
            .and_then(|pid| (self.session_of)(pid));
        let tool = match (app, &session) {
            (true, _) => "app",
            (false, Some(_)) => "claude",
            (false, None) => "shell",
        };
        let mut who = json!({"tier": if app { "app" } else { "agent" }, "tool": tool, "verified": session.is_some()});
        if let Some(session) = session {
            who["session"] = json!(session);
        }
        let switches: serde_json::Map<String, Value> = SWITCH_NAMES
            .iter()
            .map(|name| (name.to_string(), json!((self.switched)(name))))
            .collect();
        let listed: Vec<Value> = crate::commands::all_ops()
            .into_iter()
            .map(|op| now_for(app, op, &*self.switched))
            .collect();
        json!({
            "api": API_VERSION,
            "daemon": self.version,
            "caller": who,
            "app": {
                "attached": self.room.can_answer(),
                "followsInbox": self.room.can_follow(topics::INBOX),
            },
            "switches": switches,
            "ops": listed,
        })
    }
}

impl OpGroup for CapabilitiesOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::DAEMON_CAPABILITIES).then_some(OpSpec::AGENT_READ)
    }

    fn call(&self, caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::DAEMON_CAPABILITIES => Ok(self.answer(caller)),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "capabilities_ops_tests.rs"]
mod tests;
