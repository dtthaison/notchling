use crate::cluster_node::ClusterNode;
use notchling_engine::cluster::policy::RULES;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{LocalCmd, NodeId, Qualified};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const NODE_FOLLOW: &str = "node.follow";
pub const VAULT_RELAYED: [&str; 3] = [ops::VAULT_LIST, ops::VAULT_SAVE, ops::VAULT_REMOVE];
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

pub fn names_other_node(op: &str, args: &Value) -> bool {
    RULES
        .iter()
        .filter(|rule| rule.op == op)
        .find_map(|rule| rule.id_field)
        .and_then(|field| args.get(field)?.as_str())
        .is_some_and(|id| Qualified::parse(id).is_some())
}

pub fn acting_class(op: &str) -> bool {
    crate::acting_ops::ACTING_OPS.contains(&op)
        || crate::agy_ops::ACTING_OPS.contains(&op)
        || matches!(
            op,
            ops::SESSION_SEND | ops::CODEX_SEND | ops::CODEX_STOP | ops::SHELL_STOP
        )
}

pub fn acting_gated(caller: &Caller, op: &str, args: &Value) -> bool {
    if caller.client == ClientKind::Node {
        return false;
    }
    match op {
        ops::SHELL_STOP => true,
        ops::CLUSTER_DISPATCH => caller.tier == Tier::App && args.get("delegate").is_none(),
        _ => acting_class(op) && names_other_node(op, args),
    }
}

pub trait Forward: Send + Sync {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>>;
}

pub struct NoNodeForward;

impl Forward for NoNodeForward {
    fn forward(&self, _caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        let named = !matches!(args.get("node"), None | Some(Value::Null));
        if !VAULT_RELAYED.contains(&op) || !named {
            return None;
        }
        let text = "this machine is in no cluster, so it has no other vault";
        Some(Err(ApiError::new(ErrorCode::Failed, text)))
    }
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

    fn to_vault(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        let wanted = match args.get("node") {
            None | Some(Value::Null) => return None,
            Some(Value::String(text)) => text.as_str(),
            Some(_) => return Some(Err(ApiError::bad_request("node is a machine id"))),
        };
        if caller.tier != Tier::App {
            return Some(Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app uses another machine's vault",
            )));
        }
        let status = self.node.status();
        let Some(vault) = status
            .pointer("/cluster/vault")
            .and_then(Value::as_str)
            .and_then(NodeId::parse)
        else {
            let text = "this machine is in no cluster, so it has no other vault";
            return Some(Err(ApiError::new(ErrorCode::Failed, text)));
        };
        let member = |id: &NodeId| {
            status
                .get("members")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|m| m.get("id").and_then(Value::as_str) == Some(id.as_str()))
                .cloned()
        };
        let named = |id: &NodeId| {
            member(id)
                .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| id.to_string())
        };
        if NodeId::parse(wanted).as_ref() != Some(&vault) {
            return Some(Err(ApiError::bad_request(
                "that machine doesn't hold the vault",
            )));
        }
        if vault == *self.node.me() {
            return None;
        }
        let online = member(&vault)
            .and_then(|m| m.get("online").and_then(Value::as_bool))
            .unwrap_or(false);
        if !online {
            let text = format!("The vault is on {}, which is offline", named(&vault));
            return Some(Err(ApiError::new(ErrorCode::Failed, text)));
        }
        let mut bare = args.clone();
        if let Some(fields) = bare.as_object_mut() {
            fields.remove("node");
        }
        let origin = Origin {
            node: self.node.me().clone(),
            tier: OriginTier::App,
        };
        Some(self.node.local(LocalCmd::Forward {
            node: vault,
            op: op.to_string(),
            args: bare.into(),
            origin,
        }))
    }
}

impl Forward for NodeForward {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        if caller.client == ClientKind::Node {
            return None;
        }
        if VAULT_RELAYED.contains(&op) {
            return self.to_vault(caller, op, args);
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
            args: bare.into(),
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
    push: Mutex<Option<Arc<dyn PromptPush>>>,
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
        let mut slot = self.push.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(push);
        }
    }

    pub fn detach(&self) {
        let push = self.push.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(push);
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
        let Some(push) = self.push.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
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
