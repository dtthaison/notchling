use crate::acting_ops::{bad, Room};
use crate::cluster_failover_setting::parse;
use crate::cluster_move_run::MoveRunner;
use crate::cluster_node::{ClusterLog, ClusterNode};
use crate::op_group::OpGroup;
use crate::switches::SwitchFlag;
use notchling_engine::cluster::join::decode_token;
use notchling_engine::cluster::main_sessions::valid_id;
use notchling_engine::cluster::ready::annotate;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{Desired, LocalCmd, NodeId};
use notchling_events::kinds::security::SwitchCause;
use notchling_events::spec::Outcome;
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

pub const CLUSTER_OPS: [&str; 10] = [
    ops::CLUSTER_STATUS,
    ops::CLUSTER_CREATE,
    ops::CLUSTER_JOIN_TOKEN,
    ops::CLUSTER_JOIN,
    ops::CLUSTER_MEMBER_SET,
    ops::CLUSTER_REMOVE,
    ops::CLUSTER_MAIN_SET,
    ops::CLUSTER_MOVE_HOME,
    ops::CLUSTER_MOVE_NOW,
    ops::CLUSTER_FAILOVER_SET,
];

pub struct ClusterOps {
    node: Result<Arc<ClusterNode>, String>,
    link: Arc<SwitchFlag>,
    room: Arc<dyn Room>,
    audit: ClusterLog,
    last_set_mb: Arc<dyn Fn() -> u64 + Send + Sync>,
    mover: Option<Arc<MoveRunner>>,
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
        audit: ClusterLog,
    ) -> ClusterOps {
        ClusterOps {
            node,
            link,
            room,
            audit,
            last_set_mb: Arc::new(|| 0),
            mover: None,
        }
    }

    pub fn with_last_set(mut self, last_set_mb: Arc<dyn Fn() -> u64 + Send + Sync>) -> ClusterOps {
        self.last_set_mb = last_set_mb;
        self
    }

    pub fn with_mover(mut self, mover: Arc<MoveRunner>) -> ClusterOps {
        self.mover = Some(mover);
        self
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
            Ok(node) => {
                let mut status = node.status();
                annotate(&mut status, (self.last_set_mb)());
                status["move"] = self.mover.as_ref().map_or(Value::Null, |m| m.status());
                status
            }
            Err(why) => {
                json!({"cluster": Value::Null, "joining": Value::Null, "me": Value::Null, "members": [], "error": why})
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
            let cause = if headless {
                SwitchCause::ClusterJoin
            } else {
                SwitchCause::User
            };
            self.audit
                .switch(self.link.name(), true, cause, Outcome::Ok);
            let data = json!({"switch": self.link.name(), "on": true});
            self.room
                .publish(topics::SETTINGS, topics::SWITCH_CHANGED, &data);
        }
        node.join(token, headless)
    }

    fn main_set(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.main.set takes {\"id\": session id, \"main\": true or false}";
        let session = text(args, "id", usage)?.to_string();
        if !valid_id(&session) {
            return Err(bad(usage));
        }
        let main = args
            .get("main")
            .and_then(Value::as_bool)
            .ok_or_else(|| bad(usage))?;
        self.node()?.local(LocalCmd::SetMain { session, main })
    }

    fn at_home(&self, op: &str, args: &Value) -> Result<Option<Value>, ApiError> {
        let node = self.node()?;
        let status = node.status();
        let me = status.get("me").and_then(Value::as_str);
        let home = status["cluster"]["home"].as_str();
        let Some(home) = home else {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "this node is in no cluster",
            ));
        };
        if Some(home) == me {
            return Ok(None);
        }
        let home = NodeId::parse(home).ok_or_else(|| bad("the cluster's primary is unknown"))?;
        let origin = Origin {
            node: node.me().clone(),
            tier: OriginTier::App,
        };
        node.local(LocalCmd::Forward {
            node: home,
            op: op.to_string(),
            args: args.clone(),
            origin,
        })
        .map(Some)
    }

    fn mover(&self) -> Result<&Arc<MoveRunner>, ApiError> {
        self.mover
            .as_ref()
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "this machine can't move the primary"))
    }

    fn move_home(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.move_home takes {\"to\": node id}";
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields.len() != 1 {
            return Err(bad(usage));
        }
        let to = text(args, "to", usage)
            .ok()
            .and_then(NodeId::parse)
            .ok_or_else(|| bad(usage))?;
        if let Some(answer) = self.at_home(ops::CLUSTER_MOVE_HOME, args)? {
            return Ok(answer);
        }
        self.mover()?.start(&to, false)
    }

    fn move_now(&self, args: &Value) -> Result<Value, ApiError> {
        if let Some(answer) = self.at_home(ops::CLUSTER_MOVE_NOW, args)? {
            return Ok(answer);
        }
        self.mover()?.move_now()
    }

    fn join_token(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let node = self.node()?;
        if caller.client != ClientKind::Node {
            return node.join_token();
        }
        let usage = "cluster.join.token from another node names who asked: {\"minter\": id}";
        let minter = text(args, "minter", usage)
            .ok()
            .and_then(NodeId::parse)
            .ok_or_else(|| bad(usage))?;
        node.join_token_for(minter)
    }

    fn failover_set(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.failover.set takes {\"value\": \"off\", \"30m\" or \"1h\"}";
        let failover = args
            .get("value")
            .and_then(parse)
            .ok_or_else(|| bad(usage))?;
        self.node()?.local(LocalCmd::SetFailover { failover })?;
        Ok(json!({"value": failover}))
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
            ops::CLUSTER_CREATE
            | ops::CLUSTER_MEMBER_SET
            | ops::CLUSTER_REMOVE
            | ops::CLUSTER_MAIN_SET
            | ops::CLUSTER_MOVE_HOME
            | ops::CLUSTER_MOVE_NOW
            | ops::CLUSTER_FAILOVER_SET => Some(OpSpec::APP),
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
            ops::CLUSTER_JOIN_TOKEN => self.join_token(caller, args),
            ops::CLUSTER_JOIN => self.join(caller, args),
            ops::CLUSTER_MEMBER_SET => self.member_set(args),
            ops::CLUSTER_MAIN_SET => self.main_set(args),
            ops::CLUSTER_MOVE_HOME => self.move_home(args),
            ops::CLUSTER_MOVE_NOW => self.move_now(args),
            ops::CLUSTER_REMOVE => {
                let node = node_arg(args, "cluster.remove takes {\"node\": id}")?;
                self.node()?.local(LocalCmd::Remove { node })
            }
            ops::CLUSTER_FAILOVER_SET => self.failover_set(args),
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
