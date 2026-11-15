use std::collections::{BTreeMap, BTreeSet};

use super::failover::{
    armed_at_current, arming_id, auto_promote_due, failover_target, Failover, Grantor,
    PrimaryLease, FENCE_LEAD_MS, RENEW_MS,
};
use super::failover_tests::{all_ready, cluster, granted_at, on_and_armed, HALF_HOUR, HOUR, MIN};
use super::id::NodeId;
use super::members::{MemberState, Term};
use super::persist::{load, save};
use super::state::{apply, start, Change, ClusterState, Command, CommandError};
use super::testkit::{id, member};
use crate::sessions::testutil::TempDir;

fn remove_arming(state: &mut ClusterState, node: &str) {
    let seen = state.holders[&id(node)];
    apply(
        state,
        Command::Released {
            node: id(node),
            seen,
        },
        400,
    )
    .expect("released");
}

#[test]
fn target_change_keeps_the_old_deadline() {
    let mut state = on_and_armed(Failover::After30m);
    let mut lease = PrimaryLease::new(0);
    let t0 = 1_000;
    let first = lease.ask(&id("m1"), t0);
    assert!(lease.granted(&id("m1"), first, Failover::After30m, state.seq));
    let m1_grant = granted_at(t0 + 200, Failover::After30m);
    assert_eq!(state.holders.len(), 1);

    let ready = |n: &NodeId| *n != id("m1");
    assert_eq!(failover_target(&state, &ready), Some(id("m2")));
    let later = t0 + 5 * MIN;
    let second = lease.ask(&id("m2"), later);
    assert!(lease.granted(&id("m2"), second, Failover::After30m, state.seq));
    let term = state.term.clone();
    let armed_id = arming_id(&state);
    apply(
        &mut state,
        Command::Armed {
            node: id("m2"),
            term,
            id: armed_id,
        },
        500,
    )
    .expect("armed m2");
    assert_eq!(state.armed.as_ref().map(|(n, _)| n.clone()), Some(id("m2")));
    assert_eq!(state.holders.len(), 2);

    let gates = lease.gates_until(&state.holders);
    assert_eq!(gates, t0 + HALF_HOUR - FENCE_LEAD_MS);
    assert!(gates + FENCE_LEAD_MS <= m1_grant.safe_after().expect("m1 deadline"));

    let release = lease.ask(&id("m1"), later + RENEW_MS);
    assert!(lease.released(&id("m1"), release, state.holders[&id("m1")]));
    assert_eq!(
        lease.releasable(&state.holders),
        vec![(id("m1"), state.holders[&id("m1")])]
    );
    remove_arming(&mut state, "m1");
    assert_eq!(
        lease.gates_until(&state.holders),
        later + HALF_HOUR - FENCE_LEAD_MS
    );
    assert_eq!(state.holders.len(), 1);
}

#[test]
fn restart_after_setting_change_waits_for_the_target() {
    let tmp = TempDir::new("cluster-failover-restart");
    let dir = tmp.path("cluster");
    let (mut state, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        1,
    )
    .expect("create");
    save(&dir, &entry, &changes).expect("save");
    let m1 = member("m1", "macos", false);
    let term = state.term.clone();
    let mut grantor = Grantor::new(0);
    let mut lease = PrimaryLease::new(0);
    for command in [
        Command::ProposeJoin {
            member: m1.clone(),
            item: "i".to_string(),
        },
        Command::Approve {
            node: m1.id.clone(),
            client: false,
            can_be_master: true,
        },
        Command::SetFailover {
            failover: Failover::After1h,
        },
        Command::Armed {
            node: m1.id.clone(),
            term,
            id: 1,
        },
        Command::SetFailover {
            failover: Failover::After30m,
        },
    ] {
        let applied = apply(&mut state, command, 2).expect("apply");
        save(
            &dir,
            applied.entry.as_ref().expect("entry"),
            &applied.changes,
        )
        .expect("save");
    }
    let seq = lease.ask(&m1.id, 1_000);
    assert!(lease.granted(&m1.id, seq, Failover::After1h, 0));
    grantor.grant(1_200, Failover::After1h);
    assert_eq!(state.armed, None);
    assert_eq!(state.holders.len(), 1);

    let reloaded = load(&dir).expect("load").expect("state");
    assert_eq!(reloaded.holders, state.holders);
    let restarted = PrimaryLease::new(9_000_000);
    let gates = restarted.gates_until(&reloaded.holders);
    assert_eq!(gates, 0);
    assert!(gates + FENCE_LEAD_MS <= grantor.safe_after().expect("deadline"));

    let mut after = restarted;
    let ask = after.ask(&m1.id, 9_000_100);
    assert!(after.granted(&m1.id, ask, Failover::After30m, reloaded.seq));
    assert_eq!(
        after.gates_until(&reloaded.holders),
        9_000_100 + HALF_HOUR - FENCE_LEAD_MS
    );

    let mut off = PrimaryLease::new(9_100_000);
    let ask = off.ask(&m1.id, 9_100_100);
    assert!(off.released(&m1.id, ask, reloaded.holders[&m1.id]));
    let mut state = reloaded;
    for (node, seen) in off.releasable(&state.holders) {
        apply(&mut state, Command::Released { node, seen }, 3).expect("released");
    }
    assert!(state.holders.is_empty());
    assert_eq!(off.gates_until(&state.holders), u64::MAX);
}

#[test]
fn a_stale_release_never_drops_a_newer_arming() {
    let mut state = on_and_armed(Failover::After30m);
    let armed_at = state.holders[&id("m1")];
    let result = apply(
        &mut state,
        Command::Released {
            node: id("m1"),
            seen: armed_at - 1,
        },
        500,
    )
    .expect("stale release");
    assert!(result.entry.is_none());
    assert!(state.holders.contains_key(&id("m1")));
    assert!(state.armed.is_some());
    remove_arming(&mut state, "m1");
    assert!(state.holders.is_empty());
    assert_eq!(state.armed, None);
}

#[test]
fn removed_or_demoted_target_never_promotes() {
    let grantor = granted_at(1_000, Failover::After30m);
    let mut removed = on_and_armed(Failover::After30m);
    assert!(auto_promote_due(
        &id("m1"),
        &removed,
        &grantor,
        false,
        10 * HOUR
    ));
    apply(&mut removed, Command::Remove { node: id("m1") }, 600).expect("remove");
    assert_eq!(removed.armed, None);
    assert!(removed.holders.contains_key(&id("m1")));
    assert!(!auto_promote_due(
        &id("m1"),
        &removed,
        &grantor,
        false,
        10 * HOUR
    ));

    let mut demoted = on_and_armed(Failover::After30m);
    apply(
        &mut demoted,
        Command::SetMember {
            node: id("m1"),
            can_be_master: Some(false),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        },
        600,
    )
    .expect("demote");
    assert_eq!(demoted.armed, None);
    assert!(!auto_promote_due(
        &id("m1"),
        &demoted,
        &grantor,
        false,
        10 * HOUR
    ));

    let mut stale_view = on_and_armed(Failover::After30m);
    stale_view
        .members
        .get_mut(&id("m1"))
        .expect("member")
        .can_be_master = false;
    assert!(stale_view.armed.is_some());
    assert!(!armed_at_current(&stale_view));
    assert!(!auto_promote_due(
        &id("m1"),
        &stale_view,
        &grantor,
        false,
        10 * HOUR
    ));
    let mut gone = on_and_armed(Failover::After30m);
    gone.members.get_mut(&id("m1")).expect("member").state = MemberState::Removed;
    assert!(!auto_promote_due(
        &id("m1"),
        &gone,
        &grantor,
        false,
        10 * HOUR
    ));
}

#[test]
fn pre_restart_echo_never_matches() {
    let mut before = PrimaryLease::new(1_000_000);
    let old: Vec<u64> = (0..3).map(|i| before.ask(&id("m1"), i * 30_000)).collect();
    let mut after = PrimaryLease::new(1_000_000 + 60_000);
    let fresh = after.ask(&id("m1"), 5_000);
    assert!(!old.contains(&fresh));
    for seq in old {
        assert!(!after.granted(&id("m1"), seq, Failover::After1h, 0));
    }
    assert_eq!(after.gates_until(&BTreeMap::new()), u64::MAX);
    assert!(after.granted(&id("m1"), fresh, Failover::After30m, 0));
}

#[test]
fn term_change_keeps_holders_but_not_the_new_home() {
    let mut state = on_and_armed(Failover::After30m);
    let term = state.term.clone();
    let armed_id = arming_id(&state);
    apply(
        &mut state,
        Command::Armed {
            node: id("m2"),
            term,
            id: armed_id,
        },
        650,
    )
    .expect("armed m2");
    assert_eq!(state.holders.len(), 2);
    apply(
        &mut state,
        Command::Term {
            term: Term {
                n: 2,
                node: id("m1"),
            },
        },
        700,
    )
    .expect("term");
    assert_eq!(state.armed, None);
    assert_eq!(
        state.holders.keys().cloned().collect::<Vec<_>>(),
        [id("m2")]
    );
    let fresh = PrimaryLease::new(5_000_000);
    assert_eq!(fresh.gates_until(&state.holders), 0);
    assert!(arming_id(&state) > armed_id);
}

#[test]
fn arming_ids_only_grow_per_node() {
    let mut state = on_and_armed(Failover::After30m);
    let held = state.holders[&id("m1")];
    let term = state.term.clone();
    let lower = apply(
        &mut state,
        Command::Armed {
            node: id("m1"),
            term: term.clone(),
            id: held - 1,
        },
        650,
    );
    assert_eq!(lower.err(), Some(CommandError::BadTransition));
    let next_id = arming_id(&state);
    let newer = apply(
        &mut state,
        Command::Armed {
            node: id("m1"),
            term,
            id: next_id,
        },
        660,
    )
    .expect("newer");
    assert!(newer.entry.is_some());
    assert!(state.holders[&id("m1")] > held);
}

#[test]
fn an_old_armed_row_without_holders_seeds_them_on_load() {
    let tmp = TempDir::new("cluster-failover-old-row");
    let dir = tmp.path("cluster");
    let (mut state, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        1,
    )
    .expect("create");
    save(&dir, &entry, &changes).expect("save");
    let m1 = member("m1", "macos", false);
    let term = state.term.clone();
    for command in [
        Command::ProposeJoin {
            member: m1.clone(),
            item: "i".to_string(),
        },
        Command::Approve {
            node: m1.id.clone(),
            client: false,
            can_be_master: true,
        },
        Command::SetFailover {
            failover: Failover::After30m,
        },
        Command::Armed {
            node: m1.id.clone(),
            term,
            id: 3,
        },
    ] {
        let applied = apply(&mut state, command, 2).expect("apply");
        let kept: Vec<Change> = applied
            .changes
            .into_iter()
            .filter(|c| !matches!(c, Change::Holders(_)))
            .collect();
        save(&dir, applied.entry.as_ref().expect("entry"), &kept).expect("save");
    }
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded.armed, state.armed);
    assert_eq!(loaded.holders, BTreeMap::from([(m1.id, 0)]));
}

#[test]
fn an_off_hold_counts_only_once_released() {
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    assert!(lease.granted(&id("m1"), seq, Failover::Off, 0));
    let held = BTreeMap::from([(id("m1"), 5)]);
    assert_eq!(lease.gates_until(&held), 0);
    assert_eq!(lease.releasable(&held), vec![(id("m1"), 0)]);
    assert_eq!(lease.gates_until(&BTreeMap::new()), u64::MAX);
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }

    fn pick(&mut self) -> NodeId {
        id(["m1", "m2"][usize::try_from(self.below(2)).expect("small")])
    }
}

struct Peer {
    view: ClusterState,
    grantor: Grantor,
    cut: bool,
    heard: usize,
}

struct Promoted {
    node: NodeId,
    state: ClusterState,
    lease: PrimaryLease,
    known: BTreeSet<NodeId>,
}

struct World {
    primary: ClusterState,
    journal: Vec<Command>,
    peers: BTreeMap<NodeId, Peer>,
    lease: PrimaryLease,
    now: u64,
    restarts: u64,
    promoted: Option<Promoted>,
    rng: Rng,
}

impl World {
    fn new(seed: u64) -> World {
        let primary = cluster();
        let mut rng = Rng(seed);
        let peers = ["m1", "m2"]
            .iter()
            .map(|n| {
                let mut view = primary.clone();
                view.seq += rng.below(1_000);
                (
                    id(n),
                    Peer {
                        view,
                        grantor: Grantor::new(0),
                        cut: false,
                        heard: 0,
                    },
                )
            })
            .collect();
        World {
            promoted: None,
            primary,
            journal: Vec::new(),
            peers,
            lease: PrimaryLease::new(0),
            now: 1_000,
            restarts: 0,
            rng,
        }
    }

    fn push(&mut self, command: Command) {
        if let Ok(applied) = apply(&mut self.primary, command.clone(), 0) {
            if applied.entry.is_some() {
                self.journal.push(command);
            }
        }
    }

    fn hear(&mut self, node: &NodeId, all: bool) {
        let limit = self.journal.len();
        let extra = usize::try_from(self.rng.below(3)).expect("small");
        let Some(peer) = self.peers.get_mut(node) else {
            return;
        };
        if peer.cut && self.promoted.as_ref().is_some_and(|p| p.node == *node) {
            return;
        }
        let heard = peer.heard;
        let upto = if all {
            limit
        } else {
            (heard + 1 + extra).min(limit)
        };
        for command in self.journal.iter().take(upto).skip(heard) {
            let _ = apply(&mut peer.view, command.clone(), 0);
        }
        peer.heard = upto.max(heard);
    }

    fn tick(&mut self) {
        self.now += RENEW_MS;
        let mut asks: BTreeSet<NodeId> = self.primary.holders.keys().cloned().collect();
        let target = failover_target(&self.primary, &all_ready);
        asks.extend(target.clone());
        for node in asks {
            let seq = self.lease.ask(&node, self.now);
            if self.peers[&node].cut || self.rng.below(4) == 0 {
                continue;
            }
            let all = self.rng.below(3) != 0;
            self.hear(&node, all);
            let now = self.now;
            let peer = self.peers.get_mut(&node).expect("peer");
            peer.grantor.grant(now, peer.view.failover);
            let seen = peer.view.holders.get(&node).copied().unwrap_or(0);
            let reported = peer.view.failover;
            if self.rng.below(4) == 0 {
                continue;
            }
            if self.lease.granted(&node, seq, reported, seen) {
                if reported == Failover::Off {
                    self.push(Command::Released { node, seen });
                } else if reported == self.primary.failover && Some(&node) == target.as_ref() {
                    let term = self.primary.term.clone();
                    let id = arming_id(&self.primary);
                    self.push(Command::Armed { node, term, id });
                }
            }
        }
    }

    fn step(&mut self) {
        match self.rng.below(11) {
            0..=3 => self.tick(),
            4 => {
                let failover = [Failover::Off, Failover::After30m, Failover::After1h]
                    [usize::try_from(self.rng.below(3)).expect("small")];
                self.push(Command::SetFailover { failover });
            }
            5 => {
                let node = self.rng.pick();
                let on = self.rng.below(2) == 0;
                self.push(Command::SetMember {
                    node,
                    can_be_master: Some(on),
                    client: None,
                    reference: None,
                    accepts_delegation: None,
                    headless: None,
                    desired: None,
                });
            }
            6 => {
                let node = self.rng.pick();
                let peer = self.peers.get_mut(&node).expect("peer");
                peer.cut = !peer.cut;
            }
            7 => {
                self.restarts += 1;
                self.lease = PrimaryLease::new(self.now + self.restarts * 1_000_000_000);
            }
            8 => {
                let node = self.rng.pick();
                if !self.peers[&node].cut {
                    let all = self.rng.below(2) == 0;
                    self.hear(&node, all);
                }
            }
            9 => self.promote(),
            _ => self.now += self.rng.below(40 * MIN),
        }
    }

    fn promote(&mut self) {
        if self.promoted.is_some() {
            return;
        }
        let node = self.rng.pick();
        let now = self.now;
        let peer = self.peers.get_mut(&node).expect("peer");
        if !auto_promote_due(&node, &peer.view, &peer.grantor, false, now) {
            return;
        }
        let known: BTreeSet<NodeId> = peer
            .view
            .holders
            .keys()
            .filter(|n| **n != node)
            .cloned()
            .collect();
        let term = Term {
            n: peer.view.term.n + 1,
            node: node.clone(),
        };
        apply(&mut peer.view, Command::Term { term }, 0).expect("term");
        peer.cut = true;
        self.promoted = Some(Promoted {
            node,
            state: peer.view.clone(),
            lease: PrimaryLease::new(now),
            known,
        });
    }

    fn check(&self, label: &str) {
        if let Some(promoted) = &self.promoted {
            let gates = promoted.lease.gates_until(&promoted.state.holders);
            for (node, peer) in &self.peers {
                let Some(at) = peer.grantor.safe_after() else {
                    continue;
                };
                if *node != promoted.node
                    && promoted.known.contains(node)
                    && auto_promote_due(node, &peer.view, &peer.grantor, false, at)
                {
                    assert!(
                        gates.saturating_add(FENCE_LEAD_MS) <= at,
                        "{label}: {} promoted while {node} may promote at {at}, its gates close at {gates}",
                        promoted.node
                    );
                }
            }
        }
        let gates = self.lease.gates_until(&self.primary.holders);
        for (node, peer) in &self.peers {
            let Some(at) = peer.grantor.safe_after() else {
                continue;
            };
            if auto_promote_due(node, &peer.view, &peer.grantor, false, at) {
                assert!(
                    gates.saturating_add(FENCE_LEAD_MS) <= at,
                    "{label}: {node} may promote at {at}, gates close at {gates}"
                );
            }
        }
    }
}

#[test]
fn every_node_that_may_promote_does_so_after_the_primary_gates_close() {
    let mut promotes = 0;
    for seed in 0..400 {
        let mut world = World::new(seed);
        for step in 0..160 {
            world.step();
            world.check(&format!("seed {seed} step {step}"));
        }
        promotes += usize::from(world.promoted.is_some());
    }
    assert!(promotes >= 20, "{promotes}");
}
