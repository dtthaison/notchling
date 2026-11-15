use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map};

use super::invariants::Violation;
use super::world::{Event, World};
use crate::cluster::core::{Input, LocalCmd, SnapAsk};
use crate::cluster::id::NodeId;
use crate::cluster::members::MemberState;
use crate::cluster::snapshot::{Observed, SessionState};
use crate::cluster::wire::{Msg, Origin, OriginTier};
use crate::sessions::{Agent, AgentGroup, AgentTarget};

pub const OBSERVE_MS: u64 = 2_000;
const RETRY_MS: u64 = 5_000;
const ACK_ID_BASE: u64 = 1 << 40;
const REPLY_LOST_PCT: u8 = 20;

#[derive(Clone, Copy, Debug, Default)]
pub struct SnapPlant {
    pub torn_commit: bool,
    pub restore_unpushed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct FakeSession {
    pub version: u64,
    pub working: bool,
    pub pushed: bool,
    pub locked: bool,
    pub ended: bool,
    pub wrote_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FakeEntry {
    pub session: String,
    pub version: u64,
    pub movable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FakeSet {
    pub set: String,
    pub seq: u64,
    pub entries: Vec<FakeEntry>,
    pub parts: usize,
}

#[derive(Clone, Debug)]
pub(super) struct Held {
    pub set: String,
    pub seq: u64,
    pub parts: BTreeSet<usize>,
    pub total: usize,
}

impl Held {
    fn of(set: &FakeSet) -> Held {
        Held {
            set: set.set.clone(),
            seq: set.seq,
            parts: BTreeSet::new(),
            total: set.parts,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Store {
    pub current: Option<Held>,
    pub partial: Option<Held>,
}

#[derive(Clone, Debug)]
enum Step {
    Offer,
    Parts(Vec<usize>),
    Commit,
}

enum Out {
    Go(Step),
    Refused,
    Committed,
}

#[derive(Clone, Debug)]
struct Flight {
    token: u64,
    set: String,
    step: Step,
}

#[derive(Clone, Debug, Default)]
struct Slot {
    flight: Option<Flight>,
    queued: Option<String>,
    sent: Option<String>,
    failed_ms: Option<u64>,
}

#[derive(Default)]
pub(super) struct Snaps {
    pub on: bool,
    pub plant: SnapPlant,
    pub sessions: BTreeMap<String, FakeSession>,
    pub staged: BTreeMap<NodeId, Vec<FakeSet>>,
    pub stores: BTreeMap<(NodeId, NodeId), Store>,
    latest: BTreeMap<NodeId, String>,
    slots: BTreeMap<(NodeId, NodeId), Slot>,
    next_token: u64,
    next_ack: u64,
}

impl Snaps {
    pub(super) fn node_crashed(&mut self, node: &NodeId) {
        self.slots.retain(|(home, _), _| home != node);
        self.latest.remove(node);
    }
}

fn row(id: &str, session: &FakeSession) -> Agent {
    Agent {
        id: id.to_string(),
        name: id.to_string(),
        project: "repo".to_string(),
        cwd: "/work/repo".to_string(),
        group: if session.working {
            AgentGroup::Working
        } else {
            AgentGroup::Completed
        },
        label: "sim".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(format!("job-{id}")),
        suggested_reply: None,
        transcript_path: Some(format!("/sim/{id}.jsonl")),
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

pub(super) fn snap_violation(world: &World) -> Option<Violation> {
    for ((node, home), store) in &world.snaps.stores {
        let Some(held) = &store.current else {
            continue;
        };
        let staged = world.snaps.staged.get(home);
        let newest = staged
            .and_then(|sets| sets.iter().map(|s| s.seq).max())
            .unwrap_or(0);
        let ours = staged.is_some_and(|sets| sets.iter().any(|s| s.set == held.set));
        if !ours || held.seq > newest {
            return Some(Violation::StandbyAhead {
                node: node.clone(),
                set: held.set.clone(),
            });
        }
        if held.parts.len() != held.total {
            return Some(Violation::TornSet {
                node: node.clone(),
                set: held.set.clone(),
            });
        }
    }
    None
}

impl World {
    pub fn enable_snaps(&mut self, plant: SnapPlant) {
        self.snaps.on = true;
        self.snaps.plant = plant;
    }

    pub fn snap_add_session(&mut self, id: &str) {
        self.snaps.sessions.insert(
            id.to_string(),
            FakeSession {
                version: 1,
                working: false,
                pushed: true,
                locked: false,
                ended: false,
                wrote_ms: None,
            },
        );
    }

    pub fn snap_session_work(&mut self, id: &str) {
        let now = self.now;
        if let Some(s) = self.snaps.sessions.get_mut(id) {
            s.working = true;
            s.version += 1;
            s.wrote_ms = Some(now);
        }
    }

    pub fn snap_session_finish(&mut self, id: &str) {
        let now = self.now;
        if let Some(s) = self.snaps.sessions.get_mut(id) {
            if s.working {
                s.working = false;
                s.version += 1;
                s.wrote_ms = Some(now);
            }
        }
    }

    pub fn snap_session_end(&mut self, id: &str) {
        let now = self.now;
        if let Some(s) = self.snaps.sessions.get_mut(id) {
            s.working = false;
            s.ended = true;
            s.version += 1;
            s.wrote_ms = Some(now);
        }
    }

    pub fn snap_session_pushed(&mut self, id: &str, pushed: bool) {
        if let Some(s) = self.snaps.sessions.get_mut(id) {
            s.pushed = pushed;
        }
    }

    pub fn snap_git_lock(&mut self, id: &str, locked: bool) {
        if let Some(s) = self.snaps.sessions.get_mut(id) {
            s.locked = locked;
        }
    }

    pub fn snap_sessions(&self) -> Vec<String> {
        self.snaps.sessions.keys().cloned().collect()
    }

    pub fn snap_staged(&self, home: &NodeId) -> Vec<String> {
        self.snaps
            .staged
            .get(home)
            .map(|sets| sets.iter().map(|s| s.set.clone()).collect())
            .unwrap_or_default()
    }

    pub fn snap_current(&self, node: &NodeId, home: &NodeId) -> Option<String> {
        self.snaps
            .stores
            .get(&(node.clone(), home.clone()))
            .and_then(|s| s.current.as_ref())
            .map(|h| h.set.clone())
    }

    pub fn snap_versions(&self, home: &NodeId, set: &str) -> BTreeMap<String, u64> {
        self.snaps
            .staged
            .get(home)
            .and_then(|sets| sets.iter().find(|s| s.set == set))
            .map(|s| {
                s.entries
                    .iter()
                    .map(|e| (e.session.clone(), e.version))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn snap_in_flight(&self, home: &NodeId, target: &NodeId) -> bool {
        self.snaps
            .slots
            .get(&(home.clone(), target.clone()))
            .is_some_and(|slot| slot.flight.is_some())
    }

    pub fn snap_restore(&mut self, node: &NodeId) -> Vec<String> {
        if !self.is_up(node) {
            return Vec::new();
        }
        let plant = self.snaps.plant.restore_unpushed;
        let mut restored = Vec::new();
        let mut broke = None;
        for ((target, home), store) in &self.snaps.stores {
            let Some(held) = store.current.as_ref().filter(|_| target == node) else {
                continue;
            };
            let set = self
                .snaps
                .staged
                .get(home)
                .and_then(|sets| sets.iter().find(|s| s.set == held.set));
            for entry in set.iter().flat_map(|s| s.entries.iter()) {
                if !entry.movable && !plant {
                    continue;
                }
                if !entry.movable && broke.is_none() {
                    broke = Some(Violation::MovedUnpushed {
                        session: entry.session.clone(),
                        node: node.clone(),
                    });
                }
                restored.push(entry.session.clone());
            }
        }
        if self.violation.is_none() {
            self.violation = broke;
        }
        self.trace_line(node, &format!("restore {}", restored.len()));
        restored
    }

    pub(super) fn schedule_observe(&mut self, node: &NodeId) {
        let Some(gen) = self.nodes.get(node).map(|n| n.tick_gen) else {
            return;
        };
        let at = self.now + OBSERVE_MS;
        self.schedule(
            at,
            Event::Observe {
                node: node.clone(),
                gen,
            },
        );
    }

    pub(super) fn observe(&mut self, node: &NodeId, gen: u64) {
        let live = self
            .nodes
            .get(node)
            .is_some_and(|n| n.alive && !n.suspended && n.tick_gen == gen);
        if !live {
            return;
        }
        self.schedule_observe(node);
        let Some((wall, skew)) = self.nodes.get(node).map(|n| (self.wall_of(n), n.skew)) else {
            return;
        };
        let home = self.core(node).is_some_and(|c| c.is_home());
        if home {
            let agents: Vec<Agent> = self
                .snaps
                .sessions
                .iter()
                .filter(|(_, s)| !s.ended)
                .map(|(id, s)| row(id, s))
                .collect();
            self.local(
                node,
                LocalCmd::MainSeen {
                    agents,
                    codex_exec: BTreeSet::new(),
                    respawn: Default::default(),
                },
            );
        }
        let mut observed = Observed {
            now_ms: wall,
            ..Observed::default()
        };
        for (id, s) in &self.snaps.sessions {
            let state = if s.ended {
                SessionState::Ended
            } else if s.working {
                SessionState::Working
            } else {
                SessionState::Idle
            };
            observed.session_state.insert(id.clone(), state);
            observed.git_locks.insert(id.clone(), s.locked);
            if let Some(at) = s.wrote_ms {
                observed.last_write_ms.insert(id.clone(), at as i64 + skew);
            }
        }
        self.step(node, Input::Observed { observed });
        self.snap_tick(node);
    }

    pub(super) fn snap_stage(&mut self, home: &NodeId, ask: SnapAsk) {
        let entries: Vec<FakeEntry> = ask
            .main
            .iter()
            .filter_map(|m| {
                self.snaps.sessions.get(&m.id).map(|s| FakeEntry {
                    session: m.id.clone(),
                    version: s.version,
                    movable: s.pushed,
                })
            })
            .collect();
        let set = FakeSet {
            set: format!("s-{}", ask.seq),
            seq: ask.seq,
            parts: entries.len() + 2,
            entries,
        };
        let name = set.set.clone();
        self.snaps.staged.entry(home.clone()).or_default().push(set);
        self.snaps.latest.insert(home.clone(), name.clone());
        for target in &ask.targets {
            let online = self.core(home).is_some_and(|c| c.online().contains(target));
            if online {
                self.snap_queue(home, target, name.clone());
            }
        }
    }

    fn snap_queue(&mut self, home: &NodeId, target: &NodeId, set: String) {
        let key = (home.clone(), target.clone());
        let slot = self.snaps.slots.entry(key).or_default();
        if slot.flight.is_some() {
            slot.queued = Some(set);
            return;
        }
        self.snap_start(home, target, set);
    }

    fn snap_start(&mut self, home: &NodeId, target: &NodeId, set: String) {
        self.snaps.next_token += 1;
        let token = self.snaps.next_token;
        let slot = self
            .snaps
            .slots
            .entry((home.clone(), target.clone()))
            .or_default();
        slot.flight = Some(Flight {
            token,
            set,
            step: Step::Offer,
        });
        self.snap_schedule(home, target, token);
    }

    fn snap_schedule(&mut self, home: &NodeId, target: &NodeId, token: u64) {
        let at = self.now + 100 + self.rng.below(600);
        self.schedule(
            at,
            Event::SnapStep {
                home: home.clone(),
                target: target.clone(),
                token,
            },
        );
    }

    fn snap_tick(&mut self, home: &NodeId) {
        let Some(core) = self.core(home).filter(|c| c.is_home()) else {
            return;
        };
        let online = core.online();
        let targets: Vec<NodeId> = core
            .state()
            .map(|s| {
                s.members
                    .values()
                    .filter(|m| {
                        m.id != *home
                            && m.state == MemberState::Active
                            && m.can_be_master
                            && online.contains(&m.id)
                    })
                    .map(|m| m.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let Some(latest) = self.snaps.latest.get(home).cloned() else {
            return;
        };
        for target in targets {
            let slot = self
                .snaps
                .slots
                .entry((home.clone(), target.clone()))
                .or_default();
            let waiting = slot
                .failed_ms
                .is_some_and(|at| self.now.saturating_sub(at) < RETRY_MS);
            if slot.flight.is_none() && !waiting && slot.sent.as_ref() != Some(&latest) {
                self.snap_start(home, &target, latest.clone());
            }
        }
    }

    fn snap_reachable(&self, home: &NodeId, target: &NodeId) -> Result<(), String> {
        if !self.is_up(home) || !self.is_up(target) {
            return Err("a machine is down".to_string());
        }
        if !self.net.linked(home, target) {
            return Err("the link is cut".to_string());
        }
        if !self.core(home).is_some_and(|c| c.is_home()) {
            return Err("the sender is not primary".to_string());
        }
        let accepts = self.core(target).and_then(|c| c.state()).is_some_and(|s| {
            s.home == *home
                && s.members
                    .get(target)
                    .is_some_and(|m| m.state == MemberState::Active && m.can_be_master)
        });
        if !accepts {
            return Err("the target refuses".to_string());
        }
        Ok(())
    }

    fn snap_done(&mut self, home: &NodeId, target: &NodeId, sent: Option<String>) {
        let now = self.now;
        let key = (home.clone(), target.clone());
        let next = {
            let slot = self.snaps.slots.entry(key).or_default();
            slot.flight = None;
            match sent {
                Some(set) => {
                    slot.sent = Some(set);
                    slot.failed_ms = None;
                }
                None => slot.failed_ms = Some(now),
            }
            slot.queued.take()
        };
        if let Some(set) = next {
            self.snap_start(home, target, set);
        }
    }

    fn snap_ack(&mut self, home: &NodeId, target: &NodeId, set: &FakeSet) {
        let Some(term) = self.core(target).and_then(|c| c.highest_term().cloned()) else {
            return;
        };
        let lengths: Map<String, serde_json::Value> = set
            .entries
            .iter()
            .map(|e| (format!("/sim/{}.jsonl", e.session), json!(e.version)))
            .collect();
        self.snaps.next_ack += 1;
        let msg = Msg::Call {
            id: ACK_ID_BASE + self.snaps.next_ack,
            op: "snap.ack".to_string(),
            args: json!({"set": set.set, "lengths": lengths}),
            origin: Origin {
                node: target.clone(),
                tier: OriginTier::Node,
            },
            term,
        };
        self.send(target, home, msg);
    }

    pub(super) fn snap_step(&mut self, home: &NodeId, target: &NodeId, token: u64) {
        let key = (home.clone(), target.clone());
        let Some(flight) = self
            .snaps
            .slots
            .get(&key)
            .and_then(|s| s.flight.clone())
            .filter(|f| f.token == token)
        else {
            return;
        };
        let staged = self
            .snaps
            .staged
            .get(home)
            .and_then(|sets| sets.iter().find(|s| s.set == flight.set))
            .cloned();
        let (Ok(()), Some(set)) = (self.snap_reachable(home, target), staged) else {
            self.trace_line(home, &format!("snap cut {}", flight.set));
            self.snap_done(home, target, None);
            return;
        };
        let torn = self.snaps.plant.torn_commit;
        let store = self
            .snaps
            .stores
            .entry((target.clone(), home.clone()))
            .or_default();
        let out = match flight.step {
            Step::Offer => {
                if store.current.as_ref().is_some_and(|c| c.set == set.set) {
                    Out::Go(Step::Commit)
                } else {
                    if store.partial.as_ref().is_none_or(|p| p.set != set.set) {
                        store.partial = Some(Held::of(&set));
                    }
                    let have = store.partial.as_ref().map(|p| p.parts.clone());
                    let left: Vec<usize> = (0..set.parts)
                        .filter(|i| have.as_ref().is_none_or(|h| !h.contains(i)))
                        .collect();
                    Out::Go(if left.is_empty() {
                        Step::Commit
                    } else {
                        Step::Parts(left)
                    })
                }
            }
            Step::Parts(mut left) => {
                let index = left.remove(0);
                if let Some(p) = store.partial.as_mut().filter(|p| p.set == set.set) {
                    p.parts.insert(index);
                }
                if torn {
                    store.current = store.partial.clone();
                }
                Out::Go(if left.is_empty() {
                    Step::Commit
                } else {
                    Step::Parts(left)
                })
            }
            Step::Commit => {
                let already = store.current.as_ref().is_some_and(|c| c.set == set.set);
                let whole = store
                    .partial
                    .as_ref()
                    .is_some_and(|p| p.set == set.set && p.parts.len() == p.total);
                if already || whole {
                    if !already {
                        store.current = store.partial.take();
                    }
                    Out::Committed
                } else {
                    Out::Refused
                }
            }
        };
        self.snap_audit();
        match out {
            Out::Go(next) => {
                if let Some(f) = self
                    .snaps
                    .slots
                    .get_mut(&key)
                    .and_then(|slot| slot.flight.as_mut())
                {
                    f.step = next;
                }
                self.snap_schedule(home, target, token);
            }
            Out::Refused => self.snap_done(home, target, None),
            Out::Committed => {
                self.trace_line(target, &format!("snap commit {}", set.set));
                self.snap_ack(home, target, &set);
                let lost = self.rng.chance(REPLY_LOST_PCT);
                self.snap_done(home, target, (!lost).then(|| set.set.clone()));
            }
        }
    }

    fn snap_audit(&mut self) {
        if self.violation.is_none() {
            self.violation = snap_violation(self);
        }
    }
}
