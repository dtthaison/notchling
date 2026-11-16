use std::collections::BTreeMap;

use super::failover::{failover_target, Failover, PrimaryLease};
use super::fence::Gate;
use super::id::NodeId;
use super::state::ClusterState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSession {
    pub id: String,
    pub keys: Vec<String>,
    pub lease: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetView {
    pub set: String,
    pub movable: BTreeMap<String, bool>,
}

fn keys_of(session: &LocalSession) -> Vec<String> {
    let mut keys = session.keys.clone();
    if !keys.contains(&session.id) {
        keys.push(session.id.clone());
    }
    keys
}

fn keys_for(sessions: &[LocalSession], id: &str) -> Vec<String> {
    match sessions.iter().find(|s| s.id == id) {
        Some(session) => keys_of(session),
        None => vec![id.to_string()],
    }
}

pub fn gates(
    me: &NodeId,
    state: &ClusterState,
    sessions: &[LocalSession],
    waiting: &BTreeMap<String, String>,
    name_of: &dyn Fn(&NodeId) -> String,
) -> BTreeMap<String, Gate> {
    let mut out = BTreeMap::new();
    if state.home == *me {
        for session in sessions {
            let gate = if session.lease {
                Gate::Lease
            } else {
                Gate::Free
            };
            for key in keys_of(session) {
                out.insert(key, gate.clone());
            }
        }
    }
    for claim in state.claims.values().filter(|c| c.node != *me) {
        let to = name_of(&claim.node);
        for key in keys_for(sessions, &claim.session) {
            out.insert(key, Gate::Moved { to: to.clone() });
        }
    }
    for (session, for_node) in waiting {
        for key in keys_for(sessions, session) {
            out.insert(
                key,
                Gate::Waiting {
                    for_node: for_node.clone(),
                },
            );
        }
    }
    out
}

pub fn valid_until(state: &ClusterState, lease: &PrimaryLease) -> u64 {
    if state.failover == Failover::Off {
        return u64::MAX;
    }
    lease.gates_until(&state.holders)
}

pub fn lease_target(state: &ClusterState, ready: &dyn Fn(&NodeId) -> bool) -> Option<NodeId> {
    if state.failover == Failover::Off {
        return None;
    }
    failover_target(state, ready)
}

pub fn may_move(history: &[SetView], acked: Option<&str>, session: &str) -> bool {
    let from = match acked {
        None => 0,
        Some(set) => match history.iter().position(|view| view.set == set) {
            Some(at) => at,
            None => return true,
        },
    };
    let range = &history[from..];
    if range.is_empty() {
        return true;
    }
    !range
        .iter()
        .all(|view| view.movable.get(session) == Some(&false))
}
