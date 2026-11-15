use super::{Driver, Reply};
use crate::cluster_node::lock;
use notchling_engine::cluster::core::GRANT_REVOKE_JOB;
use notchling_engine::cluster::wire::{encode, Msg, WireError};
use notchling_engine::cluster::{Effect, NodeId};
use notchling_events::kinds::security::ClusterStep;
use notchling_events::spec::Outcome;
use notchling_local_api::topics;
use serde_json::{json, Value};

impl Driver {
    pub(super) fn revoke_unsaved(&mut self, runs: Vec<Effect>, rest: Vec<Effect>) -> Vec<Effect> {
        let (revokes, others) = revocations(runs, rest);
        for revoke in revokes {
            self.effect(revoke);
        }
        others
    }

    pub(super) fn tell(&mut self, topic: String, name: String, data: Value) {
        let msg = Msg::Event { topic, name, data };
        let Ok(line) = encode(&msg) else {
            return;
        };
        for conn in self.conns.values() {
            let _ = conn.send(line.clone());
        }
    }

    pub(super) fn publish(&mut self, topic: &str, name: &str, data: &Value) {
        if topic == topics::SESSIONS && name == topics::SESSIONS_CHANGED {
            if let Some(from) = data
                .get("from")
                .and_then(Value::as_str)
                .and_then(NodeId::parse)
            {
                self.hooks.peer_sessions(&from);
            }
            return;
        }
        if topic != topics::CLUSTER || name == topics::CLUSTER_CHANGED {
            return;
        }
        if name == topics::CLUSTER_REFUSED {
            let from = data.get("from").and_then(Value::as_str);
            self.audit
                .step(ClusterStep::Refused, from, None, Outcome::Refused);
        }
        self.room.publish(topic, name, data);
    }

    pub(super) fn status(&self) -> Value {
        let mut status = self.core.status();
        if let Some(members) = status.get_mut("members").and_then(Value::as_array_mut) {
            for member in members {
                let load = member
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(NodeId::parse)
                    .and_then(|id| self.loads.get(&id).cloned())
                    .unwrap_or(Value::Null);
                member["load"] = load;
            }
        }
        status["link"] = json!(self.link);
        status["listening"] = json!(self.listener.as_ref().map(|l| l.ports()));
        status["tailscale"] = json!(self.tailscale);
        status["error"] = json!(self.error);
        status
    }

    pub(super) fn after(&mut self) {
        *lock(&self.shared.state) = self.core.state().cloned();
        let status = self.status();
        if status == self.published {
            return;
        }
        *lock(&self.shared.status) = status.clone();
        let mut public = status.clone();
        if let Some(members) = public.get_mut("members").and_then(Value::as_array_mut) {
            for member in members {
                if let Some(m) = member.as_object_mut() {
                    m.remove("load");
                }
            }
        }
        let mut before = self.published.clone();
        if let Some(members) = before.get_mut("members").and_then(Value::as_array_mut) {
            for member in members {
                if let Some(m) = member.as_object_mut() {
                    m.remove("load");
                }
            }
        }
        self.published = status.clone();
        if public != before {
            self.room
                .publish(topics::CLUSTER, topics::CLUSTER_CHANGED, &status);
        }
    }
}

pub(super) fn answer(reply: Reply, result: Result<Value, WireError>) {
    match reply {
        Reply::Wait(tx) => {
            let _ = tx.send(result);
        }
        Reply::Token(tx, token) => {
            let _ = tx.send(result.map(|_| json!({"token": token})));
        }
        Reply::Load(_) => {}
    }
}

fn revocations(runs: Vec<Effect>, rest: Vec<Effect>) -> (Vec<Effect>, Vec<Effect>) {
    let mut revokes = Vec::new();
    let mut others = Vec::new();
    for effect in runs.into_iter().chain(rest) {
        match effect {
            Effect::RunLocal { ref op, .. } if op == GRANT_REVOKE_JOB => revokes.push(effect),
            Effect::RunLocal { .. } => {}
            other => others.push(other),
        }
    }
    (revokes, others)
}
