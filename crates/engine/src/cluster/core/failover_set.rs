use serde::Deserialize;
use serde_json::{json, Value};

use super::{Effect, NodeCore, Target};
use crate::cluster::failover::{armed_at_current, failover_target, Failover};
use crate::cluster::state::{ClusterState, Command};
use crate::cluster::wire::WireError;

pub(super) const OP: &str = "cluster.failover.set";

#[derive(Deserialize)]
struct SetArgs {
    failover: Failover,
}

impl NodeCore {
    pub(super) fn local_failover(&mut self, local: u64, failover: Failover, out: &mut Vec<Effect>) {
        if self.state.is_none() {
            let err = WireError::new("no_cluster", "this node is in no cluster");
            self.finish(Target::Local(local), Err(err), out);
        } else if self.is_home() {
            let result = self.journal_failover(failover, out);
            self.finish(Target::Local(local), result, out);
        } else {
            self.relay_to_home(local, OP, json!({"failover": failover}), out);
        }
    }

    pub(super) fn native_failover_set(
        &mut self,
        target: Target,
        args: &Value,
        out: &mut Vec<Effect>,
    ) {
        let result = match serde_json::from_value::<SetArgs>(args.clone()) {
            Ok(a) => self.journal_failover(a.failover, out),
            Err(_) => Err(WireError::new("bad_args", "not a failover value")),
        };
        self.finish(target, result, out);
    }

    fn journal_failover(
        &mut self,
        failover: Failover,
        out: &mut Vec<Effect>,
    ) -> Result<Value, WireError> {
        if !self.is_home() {
            return Err(WireError::new(
                "forbidden",
                "only the primary sets failover",
            ));
        }
        self.apply(Command::SetFailover { failover }, out)
            .map_err(|e| WireError::new("refused", e.to_string()))?;
        self.push_members(None, out);
        Ok(json!({"failover": failover}))
    }

    pub(super) fn follow_failover(
        &mut self,
        failover: Option<Failover>,
        armed: bool,
        out: &mut Vec<Effect>,
    ) {
        self.primary_armed = armed;
        let Some(failover) = failover else {
            return;
        };
        let differs = self.state.as_ref().is_some_and(|s| s.failover != failover);
        if differs {
            let _ = self.apply(Command::SetFailover { failover }, out);
        }
    }

    pub(super) fn failover_status(&self, state: &ClusterState) -> Value {
        let armed = if self.is_home() {
            armed_at_current(state)
        } else {
            self.primary_armed
        };
        json!({
            "value": state.failover,
            "armed": armed,
            "target": failover_target(state, &|_| true),
        })
    }
}
