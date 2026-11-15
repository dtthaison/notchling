use crate::cluster_node::ClusterNode;
use notchling_engine::cluster::policy::RULES;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{LocalCmd, NodeId, Qualified};
use notchling_local_api::{topics, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const NODE_FOLLOW: &str = "node.follow";
pub const FOLLOW_LEASE_MS: u64 = 60_000;

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

impl Forward for NodeForward {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        if caller.client == ClientKind::Node {
            return None;
        }
        let (to, bare) = route(op, args, self.node.me())?;
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

pub trait PromptPush: Send + Sync {
    fn push(&self, nodes: &[String], event: &str, data: &Value);
}

#[derive(Default)]
pub struct PromptFollowers {
    until: Mutex<BTreeMap<String, i64>>,
    push: OnceLock<Arc<dyn PromptPush>>,
}

impl PromptFollowers {
    pub fn follow(&self, node: &str, until_ms: i64) {
        let mut until = self.until.lock().unwrap_or_else(|e| e.into_inner());
        let slot = until.entry(node.to_string()).or_insert(i64::MIN);
        *slot = (*slot).max(until_ms);
    }

    pub fn active(&self, now_ms: i64) -> bool {
        !self.nodes(now_ms).is_empty()
    }

    pub fn nodes(&self, now_ms: i64) -> Vec<String> {
        let mut until = self.until.lock().unwrap_or_else(|e| e.into_inner());
        until.retain(|_, end| *end > now_ms);
        until.keys().cloned().collect()
    }

    pub fn attach(&self, push: Arc<dyn PromptPush>) {
        let _ = self.push.set(push);
    }

    pub fn relay(&self, now_ms: i64, topic: &str, event: &str, data: &Value) {
        if topic != topics::PROMPTS
            || !(event == topics::PROMPT_PENDING || event == topics::PROMPT_RESOLVED)
            || data.get("tool").is_some()
            || data
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| id.contains('/'))
        {
            return;
        }
        let Some(push) = self.push.get() else {
            return;
        };
        let nodes = self.nodes(now_ms);
        if nodes.is_empty() {
            return;
        }
        if event == topics::PROMPT_PENDING {
            if let Some(row) = crate::prompts::push_row(data) {
                push.push(&nodes, event, &row);
            }
            return;
        }
        push.push(&nodes, event, data);
    }
}

#[cfg(test)]
#[path = "cluster_forward_tests.rs"]
mod tests;
