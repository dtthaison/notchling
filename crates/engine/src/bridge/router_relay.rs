use super::connection::Connection;
use super::model::Session;
use super::relay::{held_json, list_json, Call, Cancelled, HeldAct, Relay, Routed};
use super::router::Router;
use super::router_ops::require;
use super::wire::{self, BridgeFailure};
use serde_json::{json, Map, Value};
use std::sync::Arc;

type Body = Map<String, Value>;

pub(crate) enum Gate {
    Go { relayed: bool },
    Answer(Value),
}

pub(crate) struct Audited<'a> {
    pub op: &'a str,
    pub session: Option<&'a str>,
    pub length: Option<usize>,
}

pub(crate) fn mark_relayed(mut data: Value, relayed: bool) -> Value {
    if relayed {
        data["relayed"] = Value::Bool(true);
    }
    data
}

impl Router {
    pub(crate) fn mark_stale(&self, data: &mut Value) {
        if let Some(at) = self.backend.relay().and_then(|relay| relay.stale_ms()) {
            data["stale"] = Value::from(at);
        }
    }

    pub(crate) fn gate(
        &self,
        connection: &Arc<Connection>,
        body: &Body,
        act: &HeldAct,
        session: Option<&Session>,
        audited: Audited<'_>,
    ) -> Result<Gate, BridgeFailure> {
        let Some(relay) = self.backend.relay().filter(|_| self.backend.acting_on()) else {
            return Ok(Gate::Go { relayed: false });
        };
        let request = body.get("id").map(Value::to_string).unwrap_or_default();
        let device = connection.device_id();
        let call = Call {
            device: &device,
            request: &request,
            act,
            session,
        };
        match relay.route(&call) {
            Routed::Local => Ok(Gate::Go { relayed: false }),
            Routed::Relayed => Ok(Gate::Go { relayed: true }),
            Routed::Held(held) => {
                self.audit(connection, audited.op, audited.session, audited.length);
                self.push_held(&relay);
                Ok(Gate::Answer(held_json(&held)))
            }
            Routed::Done(data) => {
                self.audit(connection, audited.op, audited.session, audited.length);
                Ok(Gate::Answer(data))
            }
            Routed::Fail(failure) => Err(failure),
        }
    }

    pub(crate) fn held_list(&self) -> Value {
        match self.backend.relay() {
            Some(relay) => list_json(&relay.list()),
            None => json!({"entries": []}),
        }
    }

    pub(crate) fn held_cancel(&self, body: &Body) -> Result<Value, BridgeFailure> {
        let id = require(body, "heldId")?;
        let Some(relay) = self.backend.relay() else {
            return Ok(json!({"cancelled": false}));
        };
        let answer = match relay.cancel(id) {
            Cancelled::Dropped => json!({"cancelled": true}),
            Cancelled::Delivered => json!({"cancelled": false, "delivered": true}),
            Cancelled::Unknown => json!({"cancelled": false}),
        };
        self.push_held(&relay);
        Ok(answer)
    }

    pub(crate) fn held_confirm(&self, body: &Body) -> Result<Value, BridgeFailure> {
        let id = require(body, "heldId")?;
        let Some(relay) = self.backend.relay() else {
            return Ok(json!({"confirmed": false}));
        };
        let confirmed = relay.confirm(id);
        self.push_held(&relay);
        Ok(json!({"confirmed": confirmed}))
    }

    pub(crate) fn push_held(&self, relay: &Arc<dyn Relay>) {
        self.hub.push_held(relay.as_ref());
    }

    pub(crate) fn subscribe_held(&self, connection: &Arc<Connection>, body: &Body) {
        if body.get("held").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let Some(relay) = self.backend.relay() else {
            connection.subscription().wants_held = true;
            return;
        };
        let text = wire::text(&list_json(&relay.list()));
        {
            let mut sub = connection.subscription();
            sub.wants_held = true;
            sub.last_held_json = Some(text);
        }
        connection.send_text(held_event(&relay.list()));
    }
}

pub(crate) fn held_event(list: &super::relay::HeldList) -> String {
    let mut fields = Map::new();
    if let Value::Object(map) = list_json(list) {
        fields.extend(map);
    }
    wire::event("held.changed", fields)
}
