use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::members::{MemberState, Term};
use super::state::ClusterState;

pub const FENCE_LEAD_MS: u64 = 120_000;
pub const RENEW_MS: u64 = 30_000;
pub const REACH_MS: u64 = 10_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failover {
    #[default]
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "30m")]
    After30m,
    #[serde(rename = "1h")]
    After1h,
}

impl Failover {
    pub fn delay_ms(self) -> Option<u64> {
        match self {
            Failover::Off => None,
            Failover::After30m => Some(1_800_000),
            Failover::After1h => Some(3_600_000),
        }
    }
}

pub fn failover_target(state: &ClusterState, ready: &dyn Fn(&NodeId) -> bool) -> Option<NodeId> {
    state
        .members
        .values()
        .filter(|m| {
            m.state == MemberState::Active && m.can_be_master && m.id != state.home && ready(&m.id)
        })
        .min_by(|a, b| (a.joined_ms, &a.id).cmp(&(b.joined_ms, &b.id)))
        .map(|m| m.id.clone())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Hold {
    reported: Failover,
    until: Option<u64>,
    seen: Option<ArmingId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrimaryLease {
    next_seq: u64,
    asked: BTreeMap<NodeId, (u64, u64)>,
    holds: BTreeMap<NodeId, Hold>,
}

impl PrimaryLease {
    pub fn new(seed: u64) -> PrimaryLease {
        PrimaryLease {
            next_seq: seed,
            ..PrimaryLease::default()
        }
    }

    pub fn ask(&mut self, node: &NodeId, now: u64) -> u64 {
        self.next_seq = self.next_seq.saturating_add(1);
        self.asked.insert(node.clone(), (self.next_seq, now));
        self.next_seq
    }

    pub fn granted(
        &mut self,
        node: &NodeId,
        seq: u64,
        reported: Failover,
        seen: Option<ArmingId>,
    ) -> bool {
        match self.asked.get(node) {
            Some((asked, _)) if *asked == seq => {}
            _ => return false,
        }
        let Some((_, sent)) = self.asked.remove(node) else {
            return false;
        };
        let until = reported.delay_ms().map(|d| sent.saturating_add(d));
        self.holds.insert(
            node.clone(),
            Hold {
                reported,
                until,
                seen,
            },
        );
        true
    }

    pub fn released(&mut self, node: &NodeId, seq: u64, seen: Option<ArmingId>) -> bool {
        self.granted(node, seq, Failover::Off, seen)
    }

    pub fn reported(&self, node: &NodeId) -> Option<Failover> {
        self.holds.get(node).map(|h| h.reported)
    }

    pub fn releasable(
        &self,
        holders: &BTreeMap<NodeId, ArmingId>,
    ) -> Vec<(NodeId, Option<ArmingId>)> {
        holders
            .keys()
            .filter_map(|n| {
                let hold = self.holds.get(n)?;
                (hold.reported == Failover::Off).then(|| (n.clone(), hold.seen.clone()))
            })
            .collect()
    }

    pub fn gates_until(&self, holders: &BTreeMap<NodeId, ArmingId>) -> u64 {
        let mut gate = u64::MAX;
        let known = self
            .holds
            .iter()
            .filter(|(_, h)| h.reported != Failover::Off)
            .map(|(n, _)| n);
        for node in holders.keys().chain(known) {
            match self.holds.get(node) {
                Some(Hold {
                    reported: Failover::Off,
                    ..
                }) if !holders.contains_key(node) => {}
                Some(Hold {
                    reported: Failover::After30m | Failover::After1h,
                    until: Some(until),
                    ..
                }) => gate = gate.min(until.saturating_sub(FENCE_LEAD_MS)),
                _ => return 0,
            }
        }
        gate
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grantor {
    pub booted_at: u64,
    pub last_grant: Option<u64>,
    pub applied: Failover,
    pub revoked: bool,
}

impl Grantor {
    pub fn new(booted_at: u64) -> Grantor {
        Grantor {
            booted_at,
            last_grant: None,
            applied: Failover::Off,
            revoked: false,
        }
    }

    pub fn may_grant(&self, term: &Term, home: &NodeId, highest: &Term) -> bool {
        !self.revoked && term == highest && *home == term.node
    }

    pub fn grant(&mut self, now: u64, applied: Failover) {
        self.last_grant = Some(now);
        self.applied = applied;
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    pub fn safe_after(&self) -> Option<u64> {
        let delay = self.applied.delay_ms()?;
        let last = self.last_grant?;
        Some(
            last.saturating_add(delay)
                .max(self.booted_at.saturating_add(delay)),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArmingId {
    pub term: Term,
    pub seq: u64,
}

pub fn arming_id(state: &ClusterState) -> ArmingId {
    let next = ArmingId {
        term: state.term.clone(),
        seq: state.seq + 1,
    };
    match state.holders.values().max() {
        Some(top) if *top >= next => ArmingId {
            term: top.term.clone(),
            seq: top.seq + 1,
        },
        _ => next,
    }
}

pub fn armed_at_current(state: &ClusterState) -> bool {
    state.armed.as_ref().is_some_and(|(node, term)| {
        *term == state.term
            && *node != state.home
            && state
                .members
                .get(node)
                .is_some_and(|m| m.state == MemberState::Active && m.can_be_master)
    })
}

pub fn auto_promote_due(
    me: &NodeId,
    state: &ClusterState,
    grantor: &Grantor,
    reaches_primary: bool,
    now: u64,
) -> bool {
    state.failover != Failover::Off
        && *me != state.home
        && !grantor.revoked
        && !reaches_primary
        && armed_at_current(state)
        && state.armed.as_ref().is_some_and(|(node, _)| node == me)
        && grantor.safe_after().is_some_and(|after| now >= after)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Safety {
    Safe,
    Relay,
    NeedsConfirm(String),
}

pub fn promote_safety(
    me: &NodeId,
    state: &ClusterState,
    grantor: &Grantor,
    reaches_primary: bool,
    now: u64,
) -> Safety {
    if reaches_primary {
        return Safety::Relay;
    }
    if auto_promote_due(me, state, grantor, false, now) {
        return Safety::Safe;
    }
    let primary = state
        .members
        .get(&state.home)
        .map_or_else(|| state.home.to_string(), |m| m.name.clone());
    Safety::NeedsConfirm(format!(
        "{primary} can't be reached. If it's still running, its sessions keep going until it hears of this. Work it does after its last snapshot is kept as a copy on {primary}."
    ))
}
