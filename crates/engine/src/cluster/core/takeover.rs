use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Effect, NodeCore, Target, Then, MOVE_CALL_MS};
use crate::cluster::claims::{put_claim, put_side_copy, Claim};
use crate::cluster::id::NodeId;
use crate::cluster::members::{Member, MemberState, Term};
use crate::cluster::state::{apply, Change, ClusterState, Command};
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
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(skip)]
    pub restored: Option<ClusterState>,
    #[serde(skip)]
    pub staged: bool,
}

pub const NOT_SENT: &str = "not_sent";
pub const REFUSED: &str = "refused";

fn not_sent(why: impl Into<String>) -> WireError {
    WireError::new(NOT_SENT, why)
}

fn refused(why: &str) -> WireError {
    WireError::new(REFUSED, why)
}

fn admissible(state: &ClusterState, claim: &Claim) -> bool {
    claim.term.storable()
        && state
            .members
            .get(&claim.node)
            .is_some_and(|m| m.state == MemberState::Active)
}

pub fn eligible(t: &Takeover, me: &NodeId) -> bool {
    t.members
        .iter()
        .any(|m| m.id == *me && m.state == MemberState::Active && m.can_be_master)
}

impl NodeCore {
    pub(super) fn fenced(&self) -> Result<(), WireError> {
        match &self.fence {
            Some(why) => Err(refused(why)),
            None => Ok(()),
        }
    }

    pub(super) fn move_switch(
        &mut self,
        local: u64,
        (to, sessions, set): (NodeId, Vec<String>, Option<String>),
        out: &mut Vec<Effect>,
    ) {
        let target = Target::Local(local);
        let ask = self.takeover_for(&to).map(|mut t| {
            t.sessions = sessions;
            t.set = set;
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
            set: None,
            claims: state.claims.values().cloned().collect(),
            restored: None,
            staged: false,
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
        if !eligible(&t, &me) {
            return Err(refused("this machine can't be primary"));
        }
        if t.restored.as_ref().is_some_and(|r| r.id != t.id) {
            return Err(refused("the restored database is another cluster's"));
        }
        let mut next = state.clone();
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
        if let Some(restored) = &t.restored {
            next.jobs = restored.jobs.clone();
            next.main = restored.main.clone();
            next.kept = restored.kept.clone();
            next.ended = restored.ended.clone();
            next.seq = next.seq.max(restored.seq);
        }
        let command = Command::Sync {
            id: t.id.clone(),
            name: t.name.clone(),
            term: t.term.clone(),
            home: me,
            vault: t.vault.clone(),
            members: t.members.clone(),
        };
        let applied =
            apply(&mut next, command, self.wall_ms).map_err(|e| refused(&e.to_string()))?;
        let claims_before = next.claims.clone();
        let from_set = t.restored.iter().flat_map(|r| r.claims.values());
        let admitted: Vec<Claim> = from_set
            .chain(t.claims.iter())
            .filter(|claim| admissible(&next, claim))
            .cloned()
            .collect();
        for claim in &admitted {
            put_claim(&mut next.claims, claim);
        }
        let claims_changed = next.claims != claims_before;
        let next_claims = next.claims.clone();
        if let Some(restored) = &t.restored {
            for copy in &restored.side_copies {
                put_side_copy(&mut next.side_copies, copy, self.wall_ms);
            }
        }
        let next_copies = next.side_copies.clone();
        self.state = Some(next);
        self.changed = true;
        if t.staged {
            out.push(Effect::AdoptStaged);
        }
        if let Some(entry) = applied.entry {
            let mut changes = applied.changes;
            if claims_changed || t.restored.is_some() {
                changes.push(Change::Claims(next_claims));
            }
            if t.restored.is_some() {
                changes.push(Change::SideCopies(next_copies));
            }
            if t.restored.is_some() {
                self.complete_persist(&t.members, dropped, &mut changes);
            }
            out.push(Effect::Persist { entry, changes });
        }
        self.push_members(None, out);
        Ok(json!({"term": t.term}))
    }

    fn complete_persist(
        &self,
        members: &[Member],
        dropped: Vec<NodeId>,
        changes: &mut Vec<Change>,
    ) {
        let Some(state) = &self.state else {
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
