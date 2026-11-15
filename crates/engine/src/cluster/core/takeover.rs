use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Effect, NodeCore, Target, Then, MOVE_CALL_MS};
use crate::cluster::id::NodeId;
use crate::cluster::members::{Member, MemberState, Term};
use crate::cluster::state::{Change, ClusterState, Command};
use crate::cluster::wire::WireError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Takeover {
    pub id: String,
    pub name: String,
    pub term: Term,
    pub vault: NodeId,
    pub members: Vec<Member>,
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(skip)]
    pub restored: Option<ClusterState>,
}

pub const NOT_SENT: &str = "not_sent";

fn not_sent(why: impl Into<String>) -> WireError {
    WireError::new(NOT_SENT, why)
}

fn refused(why: &str) -> WireError {
    WireError::new("refused", why)
}

impl NodeCore {
    pub(super) fn move_switch(
        &mut self,
        local: u64,
        (to, sessions): (NodeId, Vec<String>),
        out: &mut Vec<Effect>,
    ) {
        let target = Target::Local(local);
        let ask = self.takeover_for(&to).map(|mut t| {
            t.sessions = sessions;
            t
        });
        match ask {
            Err(e) => self.finish(target, Err(e), out),
            Ok(takeover) => {
                let args = serde_json::to_value(&takeover).unwrap_or(Value::Null);
                let origin = self.node_origin();
                self.call_out_within(
                    &to,
                    "move.switch".to_string(),
                    args,
                    origin,
                    (Then::Reply(target), MOVE_CALL_MS),
                    out,
                );
            }
        }
    }

    fn takeover_for(&self, to: &NodeId) -> Result<Takeover, WireError> {
        let Some(state) = &self.state else {
            return Err(not_sent("this node is in no cluster"));
        };
        if state.home != self.me {
            return Err(not_sent("only the primary hands itself over"));
        }
        let name = self.member_name(to);
        let can = state
            .members
            .get(to)
            .is_some_and(|m| m.state == MemberState::Active && m.can_be_master);
        if *to == self.me || !can {
            return Err(not_sent(format!("{name} can't take over")));
        }
        if !self.is_online(to) {
            return Err(not_sent(format!("{name} is offline")));
        }
        Ok(Takeover {
            id: state.id.clone(),
            name: state.name.clone(),
            term: Term {
                n: state.term.n + 1,
                node: to.clone(),
            },
            vault: state.vault.clone(),
            members: self.shared_members(),
            sessions: Vec::new(),
            restored: None,
        })
    }

    pub(super) fn take_primary(&mut self, target: Target, t: Takeover, out: &mut Vec<Effect>) {
        let result = self.takeover(t, out);
        self.finish(target, result, out);
    }

    fn takeover(&mut self, t: Takeover, out: &mut Vec<Effect>) -> Result<Value, WireError> {
        let me = self.me.clone();
        let Some(state) = &self.state else {
            return Err(refused("this node is in no cluster"));
        };
        if state.id != t.id {
            return Err(refused("that is another cluster"));
        }
        if t.term.node != me {
            return Err(refused("that term isn't this machine's"));
        }
        if state.home == me && state.term == t.term {
            return Ok(json!({"term": t.term}));
        }
        if t.term <= state.term {
            return Err(refused("this machine already has a newer term"));
        }
        let eligible = t
            .members
            .iter()
            .any(|m| m.id == me && m.state == MemberState::Active && m.can_be_master);
        if !eligible {
            return Err(refused("this machine can't be primary"));
        }
        let dropped: Vec<NodeId> = match &t.restored {
            Some(restored) => {
                let listed: BTreeSet<&NodeId> = t.members.iter().map(|m| &m.id).collect();
                restored
                    .members
                    .keys()
                    .filter(|id| !listed.contains(id))
                    .cloned()
                    .collect()
            }
            None => Vec::new(),
        };
        if let (Some(restored), Some(state)) = (&t.restored, &mut self.state) {
            state.jobs = restored.jobs.clone();
            state.main = restored.main.clone();
            state.kept = restored.kept.clone();
            state.ended = restored.ended.clone();
            state.seq = state.seq.max(restored.seq);
        }
        let command = Command::Sync {
            id: t.id,
            name: t.name,
            term: t.term.clone(),
            home: me,
            vault: t.vault,
            members: t.members.clone(),
        };
        let mark = out.len();
        self.apply(command, out)
            .map_err(|e| refused(&e.to_string()))?;
        if t.restored.is_some() {
            self.complete_persist(&t.members, dropped, &mut out[mark..]);
        }
        self.push_members(None, out);
        Ok(json!({"term": t.term}))
    }

    fn complete_persist(&self, members: &[Member], dropped: Vec<NodeId>, out: &mut [Effect]) {
        let Some(state) = &self.state else {
            return;
        };
        let Some(Effect::Persist { changes, .. }) = out
            .iter_mut()
            .find(|effect| matches!(effect, Effect::Persist { .. }))
        else {
            return;
        };
        changes.extend(dropped.into_iter().map(Change::Dropped));
        for member in members {
            if let Some(stored) = state.members.get(&member.id) {
                let known = changes
                    .iter()
                    .any(|c| matches!(c, Change::Member(m) if m.id == stored.id));
                if !known {
                    changes.push(Change::Member(stored.clone()));
                }
            }
        }
        changes.extend(state.jobs.values().cloned().map(Change::Job));
        changes.push(Change::Main {
            main: state.main.clone(),
            kept: state.kept.clone(),
            ended: state.ended.clone(),
        });
    }
}
