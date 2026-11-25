use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::members::Term;
use super::state::Command;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub session: String,
    pub node: NodeId,
    pub n: u64,
    pub term: Term,
    pub base_set: String,
    pub base_len: u64,
    pub touched: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SideCopy {
    pub session: String,
    pub node: NodeId,
    pub base_set: String,
    pub extra_bytes: u64,
    pub repo_changed: bool,
    pub path: Option<String>,
    pub at_ms: i64,
    pub hidden: bool,
}

fn rank(c: &Claim) -> (u64, &Term, &NodeId, bool, u64, &str) {
    (
        c.n,
        &c.term,
        &c.node,
        c.touched,
        c.base_len,
        c.base_set.as_str(),
    )
}

pub fn winner<'a>(a: &'a Claim, b: &'a Claim) -> &'a Claim {
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

pub(super) fn put_claim(claims: &mut BTreeMap<String, Claim>, incoming: &Claim) -> bool {
    match claims.get(&incoming.session) {
        Some(held) if winner(held, incoming) == held => false,
        _ => {
            claims.insert(incoming.session.clone(), incoming.clone());
            true
        }
    }
}

pub(super) fn touch(claims: &mut BTreeMap<String, Claim>, session: &str) -> Option<bool> {
    let held = claims.get_mut(session)?;
    let changed = !held.touched;
    held.touched = true;
    Some(changed)
}

fn same_key(a: &SideCopy, b: &SideCopy) -> bool {
    a.session == b.session && a.node == b.node && a.base_set == b.base_set
}

pub(super) fn put_side_copy(copies: &mut Vec<SideCopy>, incoming: &SideCopy, now_ms: i64) -> bool {
    let mut incoming = incoming.clone();
    if incoming.at_ms == 0 {
        incoming.at_ms = now_ms;
    }
    let Some(held) = copies.iter_mut().find(|c| same_key(c, &incoming)) else {
        copies.push(incoming);
        return true;
    };
    let before = held.clone();
    held.extra_bytes = held.extra_bytes.max(incoming.extra_bytes);
    held.repo_changed |= incoming.repo_changed;
    held.hidden |= incoming.hidden;
    if incoming.path.is_some() {
        held.path = incoming.path;
    }
    *held != before
}

pub(super) fn hide_side_copies(
    copies: &mut [SideCopy],
    session: &str,
    node: &NodeId,
) -> Option<bool> {
    let mut found = false;
    let mut changed = false;
    for copy in copies
        .iter_mut()
        .filter(|c| c.session == session && c.node == *node)
    {
        found = true;
        changed |= !copy.hidden;
        copy.hidden = true;
    }
    found.then_some(changed)
}

#[derive(Clone, Debug, PartialEq)]
pub enum CallGate {
    Pass,
    Record(Box<Command>),
    Foreign,
}

pub fn before_tool_call(claims: &BTreeMap<String, Claim>, me: &NodeId, session: &str) -> CallGate {
    match claims.get(session) {
        None => CallGate::Pass,
        Some(c) if c.node != *me => CallGate::Foreign,
        Some(c) if c.touched => CallGate::Pass,
        Some(_) => CallGate::Record(Box::new(Command::Touched {
            session: session.to_string(),
        })),
    }
}
