use crate::cluster_node::ClusterNode;
use notchling_engine::cluster::policy::RULES;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{LocalCmd, NodeId, Qualified};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const NODE_FOLLOW: &str = "node.follow";
pub const FOLLOW_LEASE_MS: u64 = 60_000;

const TO_NODE: [&str; 2] = [ops::INSTALL_PLAN, ops::INSTALL_APPLY];

pub fn route_to_node(
    op: &str,
    args: &Value,
    me: &NodeId,
) -> Option<Result<(NodeId, Value), ApiError>> {
    if !TO_NODE.contains(&op) {
        return None;
    }
    let named = args.get("node").filter(|n| !n.is_null())?;
    let Some(node) = named.as_str().and_then(NodeId::parse) else {
        return Some(Err(ApiError::bad_request(
            "node is the id of a machine in the cluster".to_string(),
        )));
    };
    if node == *me {
        return None;
    }
    let mut bare = args.clone();
    if let Some(fields) = bare.as_object_mut() {
        fields.remove("node");
    }
    Some(Ok((node, bare)))
}

pub fn route(op: &str, args: &Value, me: &NodeId) -> Option<(NodeId, Value)> {
    let field = RULES
        .iter()
        .filter(|rule| rule.op == op)
        .find_map(|rule| rule.id_field)?;
    let owner = Qualified::parse(args.get(field)?.as_str()?)?;
    if owner.node == *me {
        return None;
    }
    let mut bare = args.clone();
    bare[field] = Value::String(owner.id);
    Some((owner.node, bare))
}

pub trait Forward: Send + Sync {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>>;
}

pub struct NodeForward {
    node: Arc<ClusterNode>,
}

impl NodeForward {
    pub fn new(node: Arc<ClusterNode>) -> NodeForward {
        NodeForward { node }
    }
}

impl NodeForward {
    fn is_home(&self) -> bool {
        let status = self.node.status();
        let home = status.pointer("/cluster/home").and_then(Value::as_str);
        home == Some(self.node.me().as_str())
    }
}

impl Forward for NodeForward {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        if caller.client == ClientKind::Node {
            return None;
        }
        let (to, bare) = match route(op, args, self.node.me()) {
            Some(routed) => routed,
            None => match route_to_node(op, args, self.node.me())? {
                Ok(routed) if self.is_home() => routed,
                Ok(_) => {
                    return Some(Err(ApiError::new(
                        ErrorCode::Forbidden,
                        "only the primary can install on another machine",
                    )))
                }
                Err(refused) => return Some(Err(refused)),
            },
        };
        if caller.tier != Tier::App {
            return Some(Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app drives a session on another node",
            )));
        }
        let origin = Origin {
            node: self.node.me().clone(),
            tier: OriginTier::App,
        };
        Some(self.node.local(LocalCmd::Forward {
            node: to,
            op: op.to_string(),
            args: bare,
            origin,
        }))
    }
}

#[derive(Default)]
pub struct PromptFollowers {
    until: Mutex<BTreeMap<String, i64>>,
}

impl PromptFollowers {
    pub fn follow(&self, node: &str, until_ms: i64) {
        let mut until = self.until.lock().unwrap_or_else(|e| e.into_inner());
        let slot = until.entry(node.to_string()).or_insert(i64::MIN);
        *slot = (*slot).max(until_ms);
    }

    pub fn active(&self, now_ms: i64) -> bool {
        let mut until = self.until.lock().unwrap_or_else(|e| e.into_inner());
        until.retain(|_, end| *end > now_ms);
        !until.is_empty()
    }
}

#[cfg(test)]
#[path = "cluster_forward_tests.rs"]
mod tests;
