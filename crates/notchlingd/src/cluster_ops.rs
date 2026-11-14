use crate::acting_ops::{bad, Room};
use crate::cluster_node::{audit, ClusterNode};
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::cluster::join::decode_token;
use notchling_engine::cluster::{Desired, LocalCmd, NodeId};
use notchling_local_api::{ops, topics, ApiError, AuditLog, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

pub const CLUSTER_OPS: [&str; 6] = [
    ops::CLUSTER_STATUS,
    ops::CLUSTER_CREATE,
    ops::CLUSTER_JOIN_TOKEN,
    ops::CLUSTER_JOIN,
    ops::CLUSTER_MEMBER_SET,
    ops::CLUSTER_REMOVE,
];

pub struct ClusterOps {
    node: Result<Arc<ClusterNode>, String>,
    link: Arc<SwitchFlag>,
    room: Arc<dyn Room>,
    audit: AuditLog,
}

fn text<'a>(args: &'a Value, key: &str, usage: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| bad(usage))
}

fn node_arg(args: &Value, usage: &str) -> Result<NodeId, ApiError> {
    text(args, "node", usage)
        .ok()
        .and_then(NodeId::parse)
        .ok_or_else(|| bad(usage))
}

pub fn follow_switch(flag: &SwitchFlag, node: &Arc<ClusterNode>) {
    let node = Arc::downgrade(node);
    flag.subscribe(move |on| {
        if let Some(node) = node.upgrade() {
            node.set_link(on);
        }
    });
}

impl ClusterOps {
    pub fn new(
        node: Result<Arc<ClusterNode>, String>,
        link: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
        audit: AuditLog,
    ) -> ClusterOps {
        ClusterOps {
            node,
            link,
            room,
            audit,
        }
    }

    fn node(&self) -> Result<&Arc<ClusterNode>, ApiError> {
        self.node.as_ref().map_err(|why| {
            ApiError::new(
                ErrorCode::Failed,
                format!("this machine can't be a cluster node: {why}"),
            )
        })
    }

    fn status(&self) -> Value {
        match &self.node {
            Ok(node) => node.status(),
            Err(why) => {
                json!({"cluster": Value::Null, "me": Value::Null, "members": [], "error": why})
            }
        }
    }

    fn join(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let token = text(args, "token", "cluster.join takes {\"token\": \"nlj1.…\"}")?;
        decode_token(token).map_err(bad)?;
        let node = self.node()?;
        if node.status()["cluster"] != Value::Null {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "this node is already in a cluster",
            ));
        }
        let headless = caller.tier != Tier::App && !self.room.can_answer();
        if !self.link.is_on() {
            if caller.tier != Tier::App && !headless {
                return Err(self.link.refusal());
            }
            self.link.set(true);
            let by = if headless { "headless" } else { "app" };
            audit(
                &self.audit,
                "settings.set_switch cluster",
                None,
                "on",
                Some(format!("cluster.join ({by})")),
            );
            let data = json!({"switch": self.link.name(), "on": true});
            self.room
                .publish(topics::SETTINGS, topics::SWITCH_CHANGED, &data);
        }
        node.join(token, headless)
    }

    fn member_set(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.member.set takes {\"node\": id, \"canBeMaster\"?, \"client\"?, \"reference\"?, \"acceptsDelegation\"?, \"headless\"?, \"desired\"?}";
        let known = [
            "node",
            "canBeMaster",
            "client",
            "reference",
            "acceptsDelegation",
            "headless",
            "desired",
        ];
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields.keys().any(|k| !known.contains(&k.as_str())) {
            return Err(bad(usage));
        }
        let flag = |key: &str| -> Result<Option<bool>, ApiError> {
            match fields.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(usage)),
            }
        };
        let desired = match fields.get("desired") {
            None | Some(Value::Null) => None,
            Some(v) => Some(serde_json::from_value::<Desired>(v.clone()).map_err(|_| bad(usage))?),
        };
        self.node()?.local(LocalCmd::SetMember {
            node: node_arg(args, usage)?,
            can_be_master: flag("canBeMaster")?,
            client: flag("client")?,
            reference: flag("reference")?,
            accepts_delegation: flag("acceptsDelegation")?,
            headless: flag("headless")?,
            desired,
        })
    }
}

impl OpGroup for ClusterOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::CLUSTER_STATUS | ops::CLUSTER_JOIN_TOKEN | ops::CLUSTER_JOIN => {
                Some(OpSpec::AGENT)
            }
            ops::CLUSTER_CREATE | ops::CLUSTER_MEMBER_SET | ops::CLUSTER_REMOVE => {
                Some(OpSpec::APP)
            }
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CLUSTER_STATUS => Ok(self.status()),
            ops::CLUSTER_CREATE => {
                let name = text(args, "name", "cluster.create takes {\"name\": text}")?;
                self.node()?.create(name.trim())
            }
            ops::CLUSTER_JOIN_TOKEN => self.node()?.join_token(),
            ops::CLUSTER_JOIN => self.join(caller, args),
            ops::CLUSTER_MEMBER_SET => self.member_set(args),
            ops::CLUSTER_REMOVE => {
                let node = node_arg(args, "cluster.remove takes {\"node\": id}")?;
                self.node()?.local(LocalCmd::Remove { node })
            }
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &[topics::CLUSTER]
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        (topic == topics::CLUSTER).then_some(Tier::Agent)
    }
}

#[cfg(test)]
#[path = "cluster_ops_tests.rs"]
mod tests;
