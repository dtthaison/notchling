use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{json, Value};

use super::{Effect, NodeCore, Target};
use crate::cluster::ack_book::MAX_LENGTHS;
use crate::cluster::blob::safe_name;
use crate::cluster::id::NodeId;
use crate::cluster::main_sessions::MainSession;
use crate::cluster::members::{MemberState, Term};
use crate::cluster::snapshot::{carry, due, smallest_acked, Observed, Trigger};
use crate::cluster::standby_store::Acked;
use crate::cluster::wire::WireError;

#[derive(Clone, Debug, PartialEq)]
pub struct SnapAsk {
    pub seq: u64,
    pub trigger: Trigger,
    pub term: Term,
    pub home: NodeId,
    pub main: Vec<MainSession>,
    pub targets: Vec<NodeId>,
    pub acked: BTreeMap<PathBuf, u64>,
}

#[derive(Default)]
pub(super) struct SnapCore {
    prev: Observed,
    seq: u64,
    last_set_ms: Option<i64>,
    acks: BTreeMap<NodeId, Acked>,
}

fn seq_of(set: &str) -> Option<u64> {
    set.strip_prefix("s-")?.parse().ok()
}

fn keep_main<V>(map: &mut BTreeMap<String, V>, main: &BTreeSet<String>) {
    map.retain(|id, _| main.contains(id));
}

impl NodeCore {
    pub fn acked(&self, node: &NodeId) -> Option<&Acked> {
        self.snap.acks.get(node)
    }

    pub fn acks(&self) -> &BTreeMap<NodeId, Acked> {
        &self.snap.acks
    }

    fn snap_targets(&self) -> Vec<NodeId> {
        let Some(state) = &self.state else {
            return Vec::new();
        };
        if state.home != self.me {
            return Vec::new();
        }
        state
            .members
            .values()
            .filter(|m| m.id != self.me && m.state == MemberState::Active && m.can_be_master)
            .map(|m| m.id.clone())
            .collect()
    }

    fn snap_ask(&mut self, trigger: Trigger, now_ms: i64) -> Option<SnapAsk> {
        let targets = self.snap_targets();
        if targets.is_empty() {
            return None;
        }
        let state = self.state.as_ref()?;
        let lengths: Vec<BTreeMap<PathBuf, u64>> = targets
            .iter()
            .map(|t| {
                self.snap
                    .acks
                    .get(t)
                    .map(|a| a.lengths.clone())
                    .unwrap_or_default()
            })
            .collect();
        let ask = SnapAsk {
            seq: (self.snap.seq + 1).max(u64::try_from(now_ms).unwrap_or(0)),
            trigger,
            term: state.term.clone(),
            home: self.me.clone(),
            main: state.main.values().cloned().collect(),
            targets,
            acked: smallest_acked(&lengths),
        };
        self.snap.seq = ask.seq;
        self.snap.last_set_ms = Some(now_ms);
        Some(ask)
    }

    pub(super) fn on_observed(&mut self, mut now: Observed, out: &mut Vec<Effect>) {
        let main: BTreeSet<String> = match &self.state {
            Some(state) if state.home == self.me => state.main.keys().cloned().collect(),
            _ => {
                self.snap.prev = Observed::default();
                self.snap.last_set_ms = None;
                return;
            }
        };
        keep_main(&mut now.session_state, &main);
        keep_main(&mut now.git_locks, &main);
        keep_main(&mut now.last_write_ms, &main);
        now.last_set_ms = self.snap.last_set_ms;
        let trigger = due(&self.snap.prev, &now);
        self.snap.prev = carry(&self.snap.prev, &now);
        let Some(trigger) = trigger else {
            return;
        };
        if let Some(plan) = self.snap_ask(trigger, now.now_ms) {
            out.push(Effect::Snapshot { plan });
        }
    }

    pub(super) fn snap_now(&mut self, target: Target, trigger: Trigger, out: &mut Vec<Effect>) {
        let result = if !self.is_home() {
            Err(WireError::new(
                "forbidden",
                "only the primary takes a snapshot",
            ))
        } else {
            match self.snap_ask(trigger, self.wall_ms) {
                Some(plan) => {
                    let seq = plan.seq;
                    out.push(Effect::Snapshot { plan });
                    Ok(json!({ "asked": seq }))
                }
                None => Err(WireError::new(
                    "failed",
                    "no Can Be Master machine to send a snapshot to",
                )),
            }
        };
        self.finish(target, result, out);
    }

    pub(super) fn record_ack(&mut self, from: &NodeId, args: &Value) {
        let known = self.state.as_ref().is_some_and(|s| {
            s.members.get(from).is_some_and(|m| {
                m.id != self.me && m.state == MemberState::Active && m.can_be_master
            })
        });
        let Some(set) = args
            .get("set")
            .and_then(Value::as_str)
            .filter(|s| safe_name(s))
        else {
            return;
        };
        let Some(raw) = args.get("lengths").and_then(Value::as_object) else {
            return;
        };
        if !known || raw.len() > MAX_LENGTHS {
            return;
        }
        let mut lengths = BTreeMap::new();
        for (path, len) in raw {
            let (path, Some(len)) = (PathBuf::from(path), len.as_u64()) else {
                return;
            };
            if !path.is_absolute() {
                return;
            }
            lengths.insert(path, len);
        }
        let older = self
            .snap
            .acks
            .get(from)
            .and_then(|a| seq_of(&a.set))
            .zip(seq_of(set))
            .is_some_and(|(have, new)| new < have);
        if older {
            return;
        }
        self.snap.acks.insert(
            from.clone(),
            Acked {
                set: set.to_string(),
                lengths,
            },
        );
    }
}
