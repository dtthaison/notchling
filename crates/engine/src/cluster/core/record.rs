use serde::Deserialize;
use serde_json::{json, Value};

use super::{Effect, NodeCore, Target};
use crate::cluster::claims::{Claim, SideCopy};
use crate::cluster::failover::Failover;
use crate::cluster::id::NodeId;
use crate::cluster::members::MemberState;
use crate::cluster::state::Command;
use crate::cluster::wire::{Origin, OriginTier, WireError};

pub(super) const OP: &str = "cluster.record";

pub(super) struct Pushed {
    pub(super) failover: Option<Failover>,
    pub(super) armed: bool,
    pub(super) claims: Vec<Claim>,
    pub(super) side_copies: Vec<SideCopy>,
}

#[derive(Deserialize)]
struct RecordArgs {
    command: Command,
}

fn recordable(command: &Command) -> bool {
    matches!(
        command,
        Command::Claim { .. }
            | Command::Touched { .. }
            | Command::SideCopy { .. }
            | Command::HideSideCopy { .. }
    )
}

fn refused(why: impl Into<String>) -> WireError {
    WireError::new("refused", why)
}

fn not_recordable() -> WireError {
    WireError::new("bad_args", "not a claim, touch or side copy")
}

impl NodeCore {
    pub(super) fn local_record(
        &mut self,
        local: u64,
        command: Command,
        tier: OriginTier,
        out: &mut Vec<Effect>,
    ) {
        let target = Target::Local(local);
        if self.state.is_none() {
            let err = WireError::new("no_cluster", "this node is in no cluster");
            self.finish(target, Err(err), out);
        } else if !recordable(&command) {
            self.finish(target, Err(not_recordable()), out);
        } else if self.is_home() {
            let me = self.me.clone();
            let result = self.record(&me, tier, command, out);
            self.finish(target, result, out);
        } else {
            let origin = Origin {
                node: self.me.clone(),
                tier,
            };
            let args = json!({"command": command});
            self.relay_to_home_as(local, OP, args, origin, out);
        }
    }

    pub(super) fn native_record(
        &mut self,
        target: Target,
        args: &Value,
        origin: &Origin,
        out: &mut Vec<Effect>,
    ) {
        let Target::Peer(from, _) = &target else {
            return;
        };
        let from = from.clone();
        let result = match serde_json::from_value::<RecordArgs>(args.clone()) {
            Ok(a) if recordable(&a.command) => self.record(&from, origin.tier, a.command, out),
            _ => Err(not_recordable()),
        };
        self.finish(target, result, out);
    }

    pub(super) fn record(
        &mut self,
        who: &NodeId,
        tier: OriginTier,
        command: Command,
        out: &mut Vec<Effect>,
    ) -> Result<Value, WireError> {
        if !self.is_home() {
            return Err(WireError::new("forbidden", "only the primary records this"));
        }
        self.admit_record(who, tier, &command)?;
        let claimed = match &command {
            Command::Claim { claim } => Some(claim.session.clone()),
            _ => None,
        };
        let changes = self
            .apply(command, out)
            .map_err(|e| refused(e.to_string()))?;
        if changes.is_empty() {
            if let Some(session) = claimed {
                if self.held_by_another(&session, who) {
                    return Err(refused(
                        "another machine holds a newer claim on that session",
                    ));
                }
            }
            return Ok(json!({"changed": false}));
        }
        self.push_members(None, out);
        Ok(json!({"changed": true}))
    }

    fn held_by_another(&self, session: &str, who: &NodeId) -> bool {
        self.state
            .as_ref()
            .and_then(|s| s.claims.get(session))
            .is_some_and(|held| held.node != *who)
    }

    fn admit_record(
        &self,
        who: &NodeId,
        tier: OriginTier,
        command: &Command,
    ) -> Result<(), WireError> {
        let Some(state) = &self.state else {
            return Err(WireError::new("no_cluster", "this node is in no cluster"));
        };
        let active = state
            .members
            .get(who)
            .is_some_and(|m| m.state == MemberState::Active);
        if !active {
            return Err(WireError::new("forbidden", "not an active member"));
        }
        match command {
            Command::Claim { claim } if claim.node != *who => {
                Err(refused("a machine claims only for itself"))
            }
            Command::Touched { session }
                if state.claims.get(session).is_none_or(|c| c.node != *who) =>
            {
                Err(refused("that session isn't claimed by this machine"))
            }
            Command::SideCopy { copy } if copy.node != *who || copy.hidden => {
                Err(refused("a machine records only its own, shown copies"))
            }
            Command::HideSideCopy { .. } if tier != OriginTier::App => {
                Err(refused("only the maintainer hides a side copy"))
            }
            Command::HideSideCopy { session, node }
                if !state
                    .side_copies
                    .iter()
                    .any(|c| c.session == *session && c.node == *node) =>
            {
                Err(refused("there is no such side copy"))
            }
            _ => Ok(()),
        }
    }

    pub(super) fn merge_pushed(&mut self, pushed: Pushed, out: &mut Vec<Effect>) {
        for claim in pushed.claims {
            let _ = self.apply(Command::Claim { claim }, out);
        }
        for copy in pushed.side_copies {
            let _ = self.apply(Command::SideCopy { copy }, out);
        }
    }
}
