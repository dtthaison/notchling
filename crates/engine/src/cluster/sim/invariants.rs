use std::collections::{BTreeMap, BTreeSet};

use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    TwoHomesInTerm { term: Term, a: NodeId, b: NodeId },
    TermWentDown { node: NodeId },
    StaleTermAccepted { node: NodeId, call: String },
    JobStartedTwice { job: String },
    JobOnTwoNodes { job: String },
    DispatchToRemoved { job: String, node: NodeId },
    KeyTwoJobs { key: String },
}

pub fn check(world: &World) -> Result<(), Violation> {
    if let Some(v) = world.violation() {
        return Err(v.clone());
    }
    let mut homes: BTreeMap<Term, NodeId> = BTreeMap::new();
    for (id, state) in world.states() {
        if state.home != state.term.node {
            return Err(Violation::TwoHomesInTerm {
                term: state.term.clone(),
                a: state.home.clone(),
                b: state.term.node.clone(),
            });
        }
        if let Some(other) = homes.get(&state.term) {
            if *other != state.home {
                return Err(Violation::TwoHomesInTerm {
                    term: state.term.clone(),
                    a: other.clone(),
                    b: state.home.clone(),
                });
            }
        }
        homes.insert(state.term.clone(), state.home.clone());
        if let Some(seen) = world.highest_seen(&id) {
            if state.term < *seen {
                return Err(Violation::TermWentDown { node: id });
            }
        }
        let mut keys = BTreeSet::new();
        for job in state.jobs.values() {
            if !keys.insert(job.key.clone()) {
                return Err(Violation::KeyTwoJobs {
                    key: job.key.clone(),
                });
            }
        }
    }
    let mut per_job: BTreeMap<(String, Term), BTreeSet<NodeId>> = BTreeMap::new();
    let mut counts: BTreeMap<(String, Term), usize> = BTreeMap::new();
    for s in &world.host().started {
        let key = (s.job.clone(), s.term.clone());
        per_job
            .entry(key.clone())
            .or_default()
            .insert(s.node.clone());
        *counts.entry(key).or_default() += 1;
    }
    for (key, nodes) in &per_job {
        if nodes.len() > 1 {
            return Err(Violation::JobOnTwoNodes { job: key.0.clone() });
        }
    }
    for (key, n) in &counts {
        if *n > 1 {
            return Err(Violation::JobStartedTwice { job: key.0.clone() });
        }
    }
    Ok(())
}
