use std::collections::{BTreeMap, BTreeSet};

use super::failover::{
    armed_at_current, arming_id, auto_promote_due, failover_target, ArmingId, Failover, Grantor,
    PrimaryLease, FENCE_LEAD_MS, RENEW_MS,
};
use super::failover_tests::{aid, granted_at, on_and_armed, HALF_HOUR, HOUR, MIN};
use super::id::NodeId;
use super::members::{MemberState, Term};
use super::persist::{load, save};
use super::state::{apply, start, Applied, Change, ClusterState, Command, CommandError};
use super::testkit::{id, member};
use crate::sessions::testutil::TempDir;

fn remove_arming(state: &mut ClusterState, node: &str) {
    let seen = state.holders.get(&id(node)).cloned();
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

fn arm_node(state: &mut ClusterState, node: &str) -> Result<(), CommandError> {
    let term = state.term.clone();
    let armed_id = arming_id(state);
    apply(
        state,
        Command::Armed {
            node: id(node),
            term,
            id: armed_id,
        },
        500,
    )
    .map(|_| ())
}

#[test]
fn target_change_keeps_the_old_deadline() {
    let mut state = on_and_armed(Failover::After30m);
    let mut lease = PrimaryLease::new(0);
    let t0 = 1_000;
    let first = lease.ask(&id("m1"), t0);
    assert!(lease.granted(
        &id("m1"),
        first,
        Failover::After30m,
        state.holders.get(&id("m1")).cloned()
    ));
    let m1_grant = granted_at(t0 + 200, Failover::After30m);
    assert_eq!(state.holders.len(), 1);

    let ready = |n: &NodeId| *n != id("m1");
    assert_eq!(failover_target(&state, &ready), Some(id("m2")));
    assert_eq!(arm_node(&mut state, "m2"), Err(CommandError::BadTransition));
    assert_eq!(state.holders.len(), 1);
    assert_eq!(state.armed.as_ref().map(|(n, _)| n.clone()), Some(id("m1")));

    let gates = lease.gates_until(&state.holders);
    assert_eq!(gates, t0 + HALF_HOUR - FENCE_LEAD_MS);
    assert!(gates + FENCE_LEAD_MS <= m1_grant.safe_after().expect("m1 deadline"));

    let later = t0 + 5 * MIN;
    let release = lease.ask(&id("m1"), later);
    assert!(lease.released(&id("m1"), release, state.holders.get(&id("m1")).cloned()));
    assert_eq!(lease.releasable(&state.holders).len(), 1);
    remove_arming(&mut state, "m1");
    assert!(state.holders.is_empty());
    arm_node(&mut state, "m2").expect("m2 may be armed once m1 is released");
    let second = lease.ask(&id("m2"), later + RENEW_MS);
    assert!(lease.granted(&id("m2"), second, Failover::After30m, None));
    assert_eq!(
        lease.gates_until(&state.holders),
        later + RENEW_MS + HALF_HOUR - FENCE_LEAD_MS
    );
}

#[test]
fn a_second_holder_is_refused_in_apply() {
    let mut state = on_and_armed(Failover::After30m);
    assert_eq!(arm_node(&mut state, "m2"), Err(CommandError::BadTransition));
    apply(&mut state, Command::Remove { node: id("m1") }, 600).expect("remove");
    assert_eq!(state.armed, None);
    assert_eq!(arm_node(&mut state, "m2"), Err(CommandError::BadTransition));
    assert!(state.holders.contains_key(&id("m1")));
    remove_arming(&mut state, "m1");
    arm_node(&mut state, "m2").expect("armed after release");
    assert_eq!(state.holders.len(), 1);
    assert!(armed_at_current(&state));
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
            id: aid(1),
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
    assert!(lease.granted(&m1.id, seq, Failover::After1h, None));
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
    assert!(after.granted(
        &m1.id,
        ask,
        Failover::After30m,
        reloaded.holders.get(&m1.id).cloned()
    ));
    assert_eq!(
        after.gates_until(&reloaded.holders),
        9_000_100 + HALF_HOUR - FENCE_LEAD_MS
    );

    let mut off = PrimaryLease::new(9_100_000);
    let ask = off.ask(&m1.id, 9_100_100);
    assert!(off.released(&m1.id, ask, reloaded.holders.get(&m1.id).cloned()));
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
    let armed_at = state.holders[&id("m1")].clone();
    let older = ArmingId {
        term: armed_at.term.clone(),
        seq: armed_at.seq - 1,
    };
    for seen in [Some(older), None] {
        let result = apply(
            &mut state,
            Command::Released {
                node: id("m1"),
                seen,
            },
            500,
        )
        .expect("stale release");
        assert!(result.entry.is_none());
        assert!(state.holders.contains_key(&id("m1")));
        assert!(state.armed.is_some());
    }
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
        assert!(!after.granted(&id("m1"), seq, Failover::After1h, None));
    }
    assert_eq!(after.gates_until(&BTreeMap::new()), u64::MAX);
    assert!(after.granted(&id("m1"), fresh, Failover::After30m, None));
}

#[test]
fn term_change_keeps_holders_but_not_the_new_home() {
    let mut state = on_and_armed(Failover::After30m);
    let before = state.holders[&id("m1")].clone();
    let mut promoted_other = state.clone();
    apply(
        &mut promoted_other,
        Command::Term {
            term: Term {
                n: 2,
                node: id("m2"),
            },
        },
        700,
    )
    .expect("term");
    assert_eq!(promoted_other.armed, None);
    assert_eq!(promoted_other.holders.get(&id("m1")), Some(&before));
    assert_eq!(
        PrimaryLease::new(5_000_000).gates_until(&promoted_other.holders),
        0
    );
    assert!(arming_id(&promoted_other) > before);

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
    assert!(state.holders.is_empty());
    assert_eq!(state.armed, None);
}

#[test]
fn arming_ids_follow_term_order_then_seq() {
    let low_seq_high_term = ArmingId {
        term: Term {
            n: 2,
            node: id("m2"),
        },
        seq: 1,
    };
    let high_seq_low_term = ArmingId {
        term: Term {
            n: 2,
            node: id("m1"),
        },
        seq: 900,
    };
    let (low, high) = if id("m1") < id("m2") {
        (high_seq_low_term, low_seq_high_term)
    } else {
        (low_seq_high_term, high_seq_low_term)
    };
    assert!(low < high);
    let next_term = ArmingId {
        term: Term {
            n: 3,
            node: id("m1"),
        },
        seq: 0,
    };
    assert!(high < next_term);
}

#[test]
fn arming_ids_only_grow_per_node() {
    let mut state = on_and_armed(Failover::After30m);
    let held = state.holders[&id("m1")].clone();
    let term = state.term.clone();
    let lower = apply(
        &mut state,
        Command::Armed {
            node: id("m1"),
            term,
            id: ArmingId {
                term: held.term.clone(),
                seq: held.seq - 1,
            },
        },
        650,
    );
    assert_eq!(lower.err(), Some(CommandError::BadTransition));
    arm_node(&mut state, "m1").expect("newer");
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
            term: term.clone(),
            id: aid(3),
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
    let seeded = ArmingId { term, seq: 0 };
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded.armed, state.armed);
    assert_eq!(loaded.holders, BTreeMap::from([(m1.id.clone(), seeded)]));

    let mut next = loaded.clone();
    let applied = apply(
        &mut next,
        Command::SetFailover {
            failover: Failover::Off,
        },
        3,
    )
    .expect("off");
    assert_eq!(next.armed, None);
    assert!(!next.holders.is_empty());
    save(
        &dir,
        applied.entry.as_ref().expect("entry"),
        &applied.changes,
    )
    .expect("save");
    let after = load(&dir).expect("load").expect("state");
    assert_eq!(after.armed, None);
    assert_eq!(after.holders, loaded.holders);
}

#[test]
fn an_off_hold_counts_only_once_released() {
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    assert!(lease.granted(&id("m1"), seq, Failover::Off, None));
    let held = BTreeMap::from([(id("m1"), aid(5))]);
    assert_eq!(lease.gates_until(&held), 0);
    assert_eq!(lease.releasable(&held), vec![(id("m1"), None)]);
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

    fn index(&mut self, n: usize) -> usize {
        usize::try_from(self.below(u64::try_from(n).expect("small"))).expect("small")
    }

    fn pick(&mut self) -> NodeId {
        id(["m1", "m2"][self.index(2)])
    }

    fn ppm(&mut self) -> i64 {
        [-50, 0, 50][self.index(3)]
    }
}

fn clock(real: u64, ppm: i64) -> u64 {
    let r = i128::from(real);
    u64::try_from(r + r * i128::from(ppm) / 1_000_000).expect("clock")
}

fn real_of(at: u64, ppm: i64) -> u64 {
    if at == u64::MAX {
        return u64::MAX;
    }
    u64::try_from(i128::from(at) * 1_000_000 / (1_000_000 + i128::from(ppm))).expect("real")
}

fn built() -> (ClusterState, Vec<Applied>) {
    let (mut state, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        10,
    )
    .expect("create");
    let mut log = vec![Applied {
        entry: Some(entry),
        changes,
    }];
    for (name, can_be_master, at) in [("m1", true, 100), ("m2", true, 200), ("w1", false, 50)] {
        let m = member(name, "macos", false);
        for command in [
            Command::ProposeJoin {
                member: m.clone(),
                item: format!("node-join:{}", m.id),
            },
            Command::Approve {
                node: m.id.clone(),
                client: false,
                can_be_master,
            },
        ] {
            log.push(apply(&mut state, command, at).expect("join"));
        }
    }
    (state, log)
}

struct Node {
    view: ClusterState,
    applied: Vec<Applied>,
    grantor: Grantor,
    cut: bool,
    follows: usize,
    heard: usize,
    ppm: i64,
    primary: Option<usize>,
}

struct Primary {
    node: NodeId,
    state: ClusterState,
    journal: Vec<Command>,
    log: Vec<Applied>,
    lease: PrimaryLease,
    ppm: i64,
    unaware: BTreeSet<NodeId>,
}

struct World {
    primaries: Vec<Primary>,
    nodes: BTreeMap<NodeId, Node>,
    now: u64,
    promotions: usize,
    automatic: usize,
    manual_terms: BTreeSet<Term>,
    restarts: u64,
    rng: Rng,
}

impl World {
    fn new(seed: u64) -> World {
        let (base, log) = built();
        let mut rng = Rng(seed);
        let nodes = ["m1", "m2"]
            .iter()
            .map(|n| {
                (
                    id(n),
                    Node {
                        view: base.clone(),
                        applied: log.clone(),
                        grantor: Grantor::new(0),
                        cut: false,
                        follows: 0,
                        heard: 0,
                        ppm: rng.ppm(),
                        primary: None,
                    },
                )
            })
            .collect();
        let ppm = rng.ppm();
        World {
            primaries: vec![Primary {
                node: id("m5"),
                state: base,
                journal: Vec::new(),
                log,
                lease: PrimaryLease::new(0),
                ppm,
                unaware: BTreeSet::new(),
            }],
            nodes,
            now: 1_000,
            promotions: 0,
            automatic: 0,
            manual_terms: BTreeSet::new(),
            restarts: 0,
            rng,
        }
    }

    fn push(&mut self, at: usize, command: Command) {
        let primary = &mut self.primaries[at];
        if let Ok(applied) = apply(&mut primary.state, command.clone(), 0) {
            if applied.entry.is_some() {
                primary.journal.push(command);
                primary.log.push(applied);
            }
        }
    }

    fn hear(&mut self, node: &NodeId, from: usize, all: bool) {
        let extra = self.rng.index(3);
        let Some(peer) = self.nodes.get_mut(node) else {
            return;
        };
        if peer.follows != from || peer.primary.is_some() {
            return;
        }
        let journal = &self.primaries[from].journal;
        let upto = if all {
            journal.len()
        } else {
            (peer.heard + 1 + extra).min(journal.len())
        };
        for command in journal.iter().take(upto).skip(peer.heard) {
            if let Ok(applied) = apply(&mut peer.view, command.clone(), 0) {
                if applied.entry.is_some() {
                    peer.applied.push(applied);
                }
            }
        }
        peer.heard = upto.max(peer.heard);
    }

    fn tick(&mut self, at: usize) {
        self.now += RENEW_MS;
        let state = self.primaries[at].state.clone();
        let nodes = &self.nodes;
        let ready = |n: &NodeId| nodes.get(n).is_some_and(|p| !p.cut && p.primary.is_none());
        let target = failover_target(&state, &ready);
        let mut asks: BTreeSet<NodeId> = state.holders.keys().cloned().collect();
        asks.extend(target.clone());
        for node in asks {
            let Some(peer) = self.nodes.get(&node) else {
                continue;
            };
            let sent = clock(self.now, self.primaries[at].ppm);
            let seq = self.primaries[at].lease.ask(&node, sent);
            if peer.cut || peer.primary.is_some() || peer.follows != at || self.rng.below(4) == 0 {
                continue;
            }
            let all = self.rng.below(3) != 0;
            self.hear(&node, at, all);
            let home = self.primaries[at].node.clone();
            let term = self.primaries[at].state.term.clone();
            let now = self.now;
            let peer = self.nodes.get_mut(&node).expect("peer");
            if !peer.grantor.may_grant(&term, &home, &peer.view.term) {
                continue;
            }
            peer.grantor.grant(clock(now, peer.ppm), peer.view.failover);
            let seen = peer.view.holders.get(&node).cloned();
            let reported = peer.view.failover;
            if self.rng.below(4) == 0 {
                continue;
            }
            let heard = self.primaries[at]
                .lease
                .granted(&node, seq, reported, seen.clone());
            if !heard {
                continue;
            }
            if reported == Failover::Off {
                self.push(at, Command::Released { node, seen });
            } else if reported == self.primaries[at].state.failover
                && Some(&node) == target.as_ref()
            {
                let id = arming_id(&self.primaries[at].state);
                self.push(at, Command::Armed { node, term, id });
            }
        }
    }

    fn promote(&mut self) {
        if self.primaries.len() >= 4 {
            return;
        }
        let node = self.rng.pick();
        let manual = self.rng.below(6) == 0;
        let all: Vec<NodeId> = self.nodes.keys().cloned().collect();
        let now = self.now;
        let peer = self.nodes.get_mut(&node).expect("peer");
        let due = auto_promote_due(
            &node,
            &peer.view,
            &peer.grantor,
            false,
            clock(now, peer.ppm),
        );
        if peer.primary.is_some() || !(due || manual) {
            return;
        }
        let unaware: BTreeSet<NodeId> = if due {
            BTreeSet::new()
        } else {
            all.into_iter()
                .filter(|n| *n != node && !peer.view.holders.contains_key(n))
                .collect()
        };
        let term = Term {
            n: peer.view.term.n + 1,
            node: node.clone(),
        };
        let applied = apply(&mut peer.view, Command::Term { term }, 0).expect("term");
        peer.applied.push(applied);
        peer.grantor.revoke();
        let at = self.primaries.len();
        peer.primary = Some(at);
        peer.follows = at;
        self.primaries.push(Primary {
            node,
            state: peer.view.clone(),
            journal: Vec::new(),
            log: peer.applied.clone(),
            lease: PrimaryLease::new(clock(now, peer.ppm)),
            ppm: peer.ppm,
            unaware,
        });
        self.promotions += 1;
        self.automatic += usize::from(due);
        if !due {
            if let Some(p) = self.primaries.last() {
                self.manual_terms.insert(p.state.term.clone());
            }
        }
    }

    fn follow(&mut self) {
        let node = self.rng.pick();
        let at = self.rng.index(self.primaries.len());
        let term = self.primaries[at].state.term.clone();
        let now = self.now;
        let peer = self.nodes.get_mut(&node).expect("peer");
        if peer.primary.is_some() || peer.cut || self.primaries[at].node == node {
            return;
        }
        if term <= peer.view.term {
            return;
        }
        let Ok(applied) = apply(&mut peer.view, Command::Term { term }, 0) else {
            return;
        };
        peer.applied.push(applied);
        peer.follows = at;
        peer.heard = 0;
        peer.grantor = Grantor::new(clock(now, peer.ppm));
    }

    fn restart(&mut self, at: usize) {
        self.restarts += 1;
        let seed = self.restarts * 1_000_000_000;
        let ppm = self.primaries[at].ppm;
        let fresh = PrimaryLease::new(clock(self.now, ppm) + seed);
        if !self.restarts.is_multiple_of(24) {
            self.primaries[at].lease = fresh;
            return;
        }
        let tmp = TempDir::new("cluster-sweep");
        let dir = tmp.path("cluster");
        let primary = &mut self.primaries[at];
        for applied in &primary.log {
            save(
                &dir,
                applied.entry.as_ref().expect("entry"),
                &applied.changes,
            )
            .expect("save");
        }
        let loaded = load(&dir).expect("load").expect("state");
        assert_eq!(loaded, primary.state, "a restart must read back the state");
        primary.state = loaded;
        primary.lease = fresh;
    }

    fn step(&mut self) {
        let at = self.rng.index(self.primaries.len());
        match self.rng.below(14) {
            0..=3 => self.tick(at),
            4 => {
                let failover =
                    [Failover::Off, Failover::After30m, Failover::After1h][self.rng.index(3)];
                self.push(at, Command::SetFailover { failover });
            }
            5 => {
                let node = self.rng.pick();
                let on = self.rng.below(2) == 0;
                self.push(
                    at,
                    Command::SetMember {
                        node,
                        can_be_master: Some(on),
                        client: None,
                        reference: None,
                        accepts_delegation: None,
                        headless: None,
                        desired: None,
                    },
                );
            }
            6 => {
                let node = self.rng.pick();
                if let Some(peer) = self.nodes.get_mut(&node) {
                    peer.cut = !peer.cut;
                }
            }
            7 => self.restart(at),
            8 => {
                let node = self.rng.pick();
                let from = self.nodes[&node].follows;
                let all = self.rng.below(2) == 0;
                if !self.nodes[&node].cut {
                    self.hear(&node, from, all);
                }
            }
            9 => self.promote(),
            10 | 12 => self.follow(),
            11 => {
                if self.rng.below(4) == 0 {
                    let node = self.rng.pick();
                    self.push(at, Command::Remove { node });
                }
            }
            _ => self.now += self.rng.below(40 * MIN),
        }
    }

    fn check(&self, label: &str) {
        for (at, primary) in self.primaries.iter().enumerate() {
            let gates = real_of(
                primary.lease.gates_until(&primary.state.holders),
                primary.ppm,
            );
            for (node, peer) in &self.nodes {
                let Some(deadline) = peer.grantor.safe_after() else {
                    continue;
                };
                if peer.primary.is_some()
                    || (primary.unaware.contains(node) && peer.view.term < primary.state.term)
                    || self
                        .manual_terms
                        .iter()
                        .any(|t| *t > primary.state.term && *t <= peer.view.term)
                    || !auto_promote_due(node, &peer.view, &peer.grantor, false, deadline)
                {
                    continue;
                }
                let promote = real_of(deadline, peer.ppm);
                let slack = promote / 10_000 + 2;
                assert!(
                    gates.saturating_add(FENCE_LEAD_MS) <= promote + slack,
                    "{label}: primary {at} ({}) gates close at {gates} but {node} may promote at {promote}; holders {:?} armed {:?}; peer view term {} armed {:?} holders {:?} primary-term {} failover {:?}/{:?}; hold {:?}",
                    primary.node, primary.state.holders, primary.state.armed, peer.view.term, peer.view.armed, peer.view.holders, primary.state.term, primary.state.failover, peer.view.failover, primary.lease
                );
            }
        }
    }
}

#[test]
fn every_node_that_may_promote_does_so_after_every_primary_gates_close() {
    let mut promotes = 0;
    let mut second = 0;
    let mut automatic = 0;
    let mut restarts = 0;
    for seed in 0..400 {
        let mut world = World::new(seed);
        for step in 0..240 {
            world.step();
            world.check(&format!("seed {seed} step {step}"));
        }
        promotes += world.promotions;
        automatic += world.automatic;
        second += usize::from(world.promotions >= 2);
        restarts += usize::try_from(world.restarts).expect("small");
    }
    assert!(promotes >= 20, "{promotes}");
    assert!(automatic >= 20, "{automatic}");
    assert!(second >= 1, "{second}");
    assert!(restarts >= 100, "{restarts}");
}
