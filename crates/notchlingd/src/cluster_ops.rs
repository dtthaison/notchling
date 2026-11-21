use crate::acting_ops::{bad, Room};
use crate::cluster_failover_setting::parse;
use crate::cluster_invite::parse_invite;
use crate::cluster_move_run::MoveRunner;
use crate::cluster_node::{ClusterLog, ClusterNode};
use crate::cluster_promote::Promoter;
use crate::cluster_returns::Returns;
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
use std::time::Duration;

pub const CLUSTER_OPS: [&str; 16] = [
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
    ops::CLUSTER_ALLOW_SET,
    ops::CLUSTER_PROMOTE,
    ops::CLUSTER_TAKE_BACK,
    ops::CLUSTER_CONTINUE_HERE,
    ops::CLUSTER_SIDE_COPY_HIDE,
    ops::CLUSTER_INVITE,
];

const ID_MAX: usize = 200;
pub const PROMOTE_WAIT: Duration = Duration::from_secs(60);

pub struct ClusterOps {
    node: Result<Arc<ClusterNode>, String>,
    link: Arc<SwitchFlag>,
    room: Arc<dyn Room>,
    audit: ClusterLog,
    last_set_mb: Arc<dyn Fn() -> u64 + Send + Sync>,
    replication: Arc<dyn Fn(i64) -> Value + Send + Sync>,
    mover: Option<Arc<MoveRunner>>,
    promoter: Option<Arc<Promoter>>,
    returns: Option<Arc<Returns>>,
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
            replication: Arc::new(|_| json!([])),
            mover: None,
            promoter: None,
            returns: None,
        }
    }

    pub fn with_last_set(mut self, last_set_mb: Arc<dyn Fn() -> u64 + Send + Sync>) -> ClusterOps {
        self.last_set_mb = last_set_mb;
        self
    }

    pub fn with_replication(
        mut self,
        replication: Arc<dyn Fn(i64) -> Value + Send + Sync>,
    ) -> ClusterOps {
        self.replication = replication;
        self
    }

    pub fn with_mover(mut self, mover: Arc<MoveRunner>) -> ClusterOps {
        self.mover = Some(mover);
        self
    }

    pub fn with_promoter(mut self, promoter: Arc<Promoter>) -> ClusterOps {
        self.promoter = Some(promoter);
        self
    }

    pub fn with_returns(mut self, returns: Arc<Returns>) -> ClusterOps {
        self.returns = Some(returns);
        self
    }

    fn returns(&self) -> Result<&Arc<Returns>, ApiError> {
        self.returns.as_ref().ok_or_else(|| {
            ApiError::new(
                ErrorCode::Failed,
                "this machine can't take back the primary",
            )
        })
    }

    fn take_back(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.take_back takes {}";
        if !args.is_null() && args.as_object().is_none_or(|o| !o.is_empty()) {
            return Err(bad(usage));
        }
        self.returns()?.take_back()
    }

    fn continue_here(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.continue_here takes {\"id\": session id}";
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields.len() != 1 {
            return Err(bad(usage));
        }
        let id = text(args, "id", usage)?;
        if id.len() > ID_MAX {
            return Err(bad(usage));
        }
        self.returns()?.continue_here(id)
    }

    fn side_copy_hide(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.side_copy.hide takes {\"session\": id, \"node\": id}";
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields.len() != 2 {
            return Err(bad(usage));
        }
        let session = text(args, "session", usage)?;
        if session.len() > ID_MAX {
            return Err(bad(usage));
        }
        let node = node_arg(args, usage)?;
        self.returns()?.hide(session, &node)
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
                status["replication"] = (self.replication)(crate::acting_ops::now_ms());
                status["move"] = self.mover.as_ref().map_or(Value::Null, |m| m.status());
                status["promote"] = self.promoter.as_ref().map_or(Value::Null, |p| p.status());
                status["waiting"] = self
                    .returns
                    .as_ref()
                    .map_or_else(|| json!([]), |r| r.waiting());
                if status["failover"].is_object() {
                    status["failover"]["takesOverAt"] = Value::Null;
                }
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
            args: args.clone().into(),
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

    fn promote(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.promote takes {\"node\": id, \"confirmed\"?: bool}";
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields
            .keys()
            .any(|k| !["node", "confirmed"].contains(&k.as_str()))
        {
            return Err(bad(usage));
        }
        let confirmed = match fields.get("confirmed") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or_else(|| bad(usage))?,
        };
        let target = node_arg(args, usage)?;
        let node = self.node()?;
        if target != *node.me() {
            let from = Origin {
                node: node.me().clone(),
                tier: OriginTier::App,
            };
            let forward = LocalCmd::Forward {
                node: target,
                op: ops::CLUSTER_PROMOTE.to_string(),
                args: args.clone().into(),
                origin: from,
            };
            return node.local_within(forward, PROMOTE_WAIT);
        }
        let promoter = self
            .promoter
            .as_ref()
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "this machine can't be promoted"))?;
        promoter.start(confirmed)
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

    fn invite(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.tier != Tier::App {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "only the Notchling app makes invites",
            ));
        }
        let invite = parse_invite(args)?;
        let node = self.node()?;
        if caller.client != ClientKind::Node {
            return node.invite(invite);
        }
        let usage = "cluster.invite from another node names who asked: {\"minter\": id}";
        let minter = text(args, "minter", usage)
            .ok()
            .and_then(NodeId::parse)
            .ok_or_else(|| bad(usage))?;
        node.invite_for(minter, invite)
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

    fn allow_set(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = "cluster.allow.set takes {\"node\": id, \"names\"?: [name, …], \"any\"?: bool}";
        let fields = args.as_object().ok_or_else(|| bad(usage))?;
        if fields
            .keys()
            .any(|k| !["node", "names", "any"].contains(&k.as_str()))
        {
            return Err(bad(usage));
        }
        let names = match fields.get("names") {
            None | Some(Value::Null) => None,
            Some(Value::Array(list)) => Some(
                list.iter()
                    .map(|n| n.as_str().map(str::to_string).ok_or_else(|| bad(usage)))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Some(_) => return Err(bad(usage)),
        };
        let any = match fields.get("any") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_bool().ok_or_else(|| bad(usage))?),
        };
        self.node()?.local(LocalCmd::SetAllow {
            node: node_arg(args, usage)?,
            names,
            any,
        })
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
            ops::CLUSTER_STATUS => Some(OpSpec::AGENT_READ),
            ops::CLUSTER_JOIN => Some(OpSpec::AGENT_ACTS),
            ops::CLUSTER_JOIN_TOKEN
            | ops::CLUSTER_CREATE
            | ops::CLUSTER_MEMBER_SET
            | ops::CLUSTER_REMOVE
            | ops::CLUSTER_MAIN_SET
            | ops::CLUSTER_MOVE_HOME
            | ops::CLUSTER_MOVE_NOW
            | ops::CLUSTER_FAILOVER_SET
            | ops::CLUSTER_ALLOW_SET
            | ops::CLUSTER_PROMOTE
            | ops::CLUSTER_TAKE_BACK
            | ops::CLUSTER_CONTINUE_HERE
            | ops::CLUSTER_SIDE_COPY_HIDE
            | ops::CLUSTER_INVITE => Some(OpSpec::APP),
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
            ops::CLUSTER_INVITE => self.invite(caller, args),
            ops::CLUSTER_MEMBER_SET => self.member_set(args),
            ops::CLUSTER_MAIN_SET => self.main_set(args),
            ops::CLUSTER_MOVE_HOME => self.move_home(args),
            ops::CLUSTER_MOVE_NOW => self.move_now(args),
            ops::CLUSTER_ALLOW_SET => self.allow_set(args),
            ops::CLUSTER_PROMOTE => self.promote(args),
            ops::CLUSTER_TAKE_BACK => self.take_back(args),
            ops::CLUSTER_CONTINUE_HERE => self.continue_here(args),
            ops::CLUSTER_SIDE_COPY_HIDE => self.side_copy_hide(args),
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
