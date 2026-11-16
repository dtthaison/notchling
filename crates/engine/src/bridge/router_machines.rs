use super::connection::Connection;
use super::router::Router;
use super::router_ops::require;
use super::wire::{self, BridgeFailure};
use serde_json::{json, Map, Value};
use std::sync::Arc;

type Refusal = (&'static str, BridgeFailure);

const ANSWER_KEYS: [&str; 4] = ["step", "relayed", "text", "needsConfirm"];

impl Router {
    pub(crate) fn cluster_machines(&self) -> Value {
        let machines = self.backend.cluster_machines();
        json!({"machines": wire::object(&machines)})
    }

    pub(crate) fn cluster_promote(
        &self,
        body: &Map<String, Value>,
        connection: &Arc<Connection>,
    ) -> Result<Value, BridgeFailure> {
        let decided = self.promote_decide(body);
        let op = match &decided {
            Ok(_) => "cluster.promote".to_string(),
            Err((reason, _)) => format!("cluster.promote:refused:{reason}"),
        };
        self.audit(connection, &op, None, None);
        decided.map_err(|(_, failure)| failure)
    }

    fn promote_decide(&self, body: &Map<String, Value>) -> Result<Value, Refusal> {
        if !self.backend.acting_on() {
            return Err((
                "acting_off",
                BridgeFailure::invalid(super::live::ACTING_OFF),
            ));
        }
        let invalid = |failure| ("invalid", failure);
        let node = require(body, "node").map_err(invalid)?;
        let confirmed = match body.get("confirmed") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => {
                return Err(invalid(BridgeFailure::invalid(
                    "confirmed must be true or false.",
                )))
            }
        };
        let answer = self
            .backend
            .cluster_promote(node, confirmed)
            .map_err(|failure| (wire::code_name(failure.code), failure))?;
        let kept: Map<String, Value> = answer
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| ANSWER_KEYS.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(Value::Object(kept))
    }
}
