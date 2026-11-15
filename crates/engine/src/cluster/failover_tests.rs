use std::collections::BTreeMap;

use super::failover::{
    auto_promote_due, failover_target, promote_safety, Failover, Grantor, PrimaryLease, Safety,
    FENCE_LEAD_MS, REACH_MS, RENEW_MS,
};
use super::id::NodeId;
use super::members::Term;
use super::persist::{load, save};
use super::state::{apply, start, ClusterState, Command, CommandError};
use super::testkit::{id, member};
use crate::sessions::testutil::TempDir;

pub(super) const MIN: u64 = 60_000;
pub(super) const HALF_HOUR: u64 = 1_800_000;
pub(super) const HOUR: u64 = 3_600_000;

fn join(state: &mut ClusterState, name: &str, can_be_master: bool, at: i64) {
    let m = member(name, "macos", false);
    apply(
        state,
        Command::ProposeJoin {
            member: m.clone(),
            item: format!("node-join:{}", m.id),
        },
        at,
    )
    .expect("propose");
    apply(
        state,
        Command::Approve {
            node: m.id,
            client: false,
            can_be_master,
        },
        at,
    )
    .expect("approve");
}

pub(super) fn cluster() -> ClusterState {
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        10,
    )
    .expect("create");
    join(&mut state, "m1", true, 100);
    join(&mut state, "m2", true, 200);
    join(&mut state, "w1", false, 50);
    state
}

fn set(state: &mut ClusterState, failover: Failover) {
    apply(state, Command::SetFailover { failover }, 300).expect("set failover");
}

fn arm(state: &mut ClusterState, name: &str) {
    let term = state.term.clone();
    apply(
        state,
        Command::Armed {
            node: id(name),
            term,
        },
        310,
    )
    .expect("armed");
}

fn held() -> BTreeMap<NodeId, u64> {
    BTreeMap::from([(id("m1"), 0)])
}

pub(super) fn all_ready(_: &NodeId) -> bool {
    true
}

pub(super) fn on_and_armed(failover: Failover) -> ClusterState {
    let mut state = cluster();
    set(&mut state, failover);
    arm(&mut state, "m1");
    state
}

pub(super) fn granted_at(now: u64, failover: Failover) -> Grantor {
    let mut grantor = Grantor::new(0);
    grantor.grant(now, failover);
    grantor
}

#[test]
fn failover_values_and_delays() {
    assert_eq!(Failover::default(), Failover::Off);
    assert_eq!(Failover::Off.delay_ms(), None);
    assert_eq!(Failover::After30m.delay_ms(), Some(HALF_HOUR));
    assert_eq!(Failover::After1h.delay_ms(), Some(HOUR));
    for (value, text) in [
        (Failover::Off, "\"off\""),
        (Failover::After30m, "\"30m\""),
        (Failover::After1h, "\"1h\""),
    ] {
        assert_eq!(serde_json::to_string(&value).expect("json"), text);
        assert_eq!(serde_json::from_str::<Failover>(text).expect("back"), value);
    }
    assert!(serde_json::from_str::<Failover>("\"2h\"").is_err());
    assert_eq!(FENCE_LEAD_MS, 120_000);
    assert_eq!(RENEW_MS, 30_000);
    assert_eq!(REACH_MS, 10_000);
}

#[test]
fn target_is_first_ready_can_be_master_by_join_order() {
    let mut state = cluster();
    assert_eq!(failover_target(&state, &all_ready), Some(id("m1")));
    join(&mut state, "m3", true, 100);
    let tied = if id("m1") < id("m3") { "m1" } else { "m3" };
    assert_eq!(failover_target(&state, &all_ready), Some(id(tied)));
    let home_only = |n: &NodeId| *n == id("m5");
    assert_eq!(failover_target(&state, &home_only), None);
}

#[test]
fn not_ready_machine_is_never_target() {
    let mut state = cluster();
    let not_m1 = |n: &NodeId| *n != id("m1");
    assert_eq!(failover_target(&state, &not_m1), Some(id("m2")));
    let nobody = |_: &NodeId| false;
    assert_eq!(failover_target(&state, &nobody), None);
    let only_worker = |n: &NodeId| *n == id("w1");
    assert_eq!(failover_target(&state, &only_worker), None);
    apply(&mut state, Command::Remove { node: id("m1") }, 400).expect("remove");
    assert_eq!(failover_target(&state, &all_ready), Some(id("m2")));
}

#[test]
fn lease_runs_from_the_ask_not_the_answer() {
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    lease.granted(&id("m1"), seq, Failover::After30m, 0);
    assert_eq!(
        lease.gates_until(&held()),
        1_000 + HALF_HOUR - FENCE_LEAD_MS
    );
    let next = lease.ask(&id("m1"), 31_000);
    assert!(next > seq);
    lease.granted(&id("m1"), next, Failover::After30m, 0);
    assert_eq!(
        lease.gates_until(&held()),
        31_000 + HALF_HOUR - FENCE_LEAD_MS
    );
    let later = lease.ask(&id("m1"), 61_000);
    lease.granted(&id("m1"), later, Failover::After1h, 0);
    assert_eq!(lease.gates_until(&held()), 61_000 + HOUR - FENCE_LEAD_MS);
}

#[test]
fn stale_echo_is_ignored() {
    let mut lease = PrimaryLease::new(0);
    let first = lease.ask(&id("m1"), 1_000);
    let second = lease.ask(&id("m1"), 31_000);
    assert_ne!(first, second);
    lease.granted(&id("m1"), first, Failover::After30m, 0);
    assert_eq!(lease.gates_until(&held()), 0);
    lease.granted(&id("m1"), second, Failover::After30m, 0);
    let deadline = lease.gates_until(&held());
    assert_eq!(deadline, 31_000 + HALF_HOUR - FENCE_LEAD_MS);
    lease.granted(&id("m1"), first, Failover::After1h, 0);
    lease.granted(&id("m1"), first, Failover::Off, 0);
    lease.granted(&id("m1"), second + 1_000, Failover::Off, 0);
    assert_eq!(lease.gates_until(&held()), deadline);
}

#[test]
fn off_never_expires() {
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    lease.granted(&id("m1"), seq, Failover::Off, 0);
    assert_eq!(lease.gates_until(&held()), u64::MAX);
    assert_eq!(lease.gates_until(&BTreeMap::new()), u64::MAX);
    let on = lease.ask(&id("m1"), 31_000);
    lease.granted(&id("m1"), on, Failover::After1h, 0);
    assert_ne!(lease.gates_until(&held()), u64::MAX);
    let off = lease.ask(&id("m1"), 61_000);
    lease.granted(&id("m1"), off, Failover::Off, 0);
    assert_eq!(lease.gates_until(&held()), u64::MAX);
}

#[test]
fn unarmed_target_never_expires() {
    let mut lease = PrimaryLease::new(0);
    assert_eq!(lease.gates_until(&BTreeMap::new()), u64::MAX);
    assert_eq!(lease.gates_until(&held()), 0);
    let seq = lease.ask(&id("m1"), 1_000);
    assert_eq!(lease.gates_until(&BTreeMap::new()), u64::MAX);
    assert_eq!(lease.gates_until(&held()), 0);
    lease.granted(&id("m1"), seq, Failover::After30m, 0);
    assert_eq!(
        lease.gates_until(&held()),
        1_000 + HALF_HOUR - FENCE_LEAD_MS
    );
}

#[test]
fn a_held_deadline_outlives_an_armed_reset() {
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    lease.granted(&id("m1"), seq, Failover::After30m, 0);
    assert_eq!(
        lease.gates_until(&BTreeMap::new()),
        1_000 + HALF_HOUR - FENCE_LEAD_MS
    );
}

#[test]
fn off_never_promotes() {
    let state = cluster();
    let grantor = granted_at(1_000, Failover::After30m);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        u64::MAX
    ));
    let mut armed = on_and_armed(Failover::After30m);
    let grantor = granted_at(1_000, Failover::After30m);
    assert!(auto_promote_due(&id("m1"), &armed, &grantor, false, HOUR));
    set(&mut armed, Failover::Off);
    assert!(!auto_promote_due(
        &id("m1"),
        &armed,
        &grantor,
        false,
        10 * HOUR
    ));
    let off_grant = granted_at(1_000, Failover::Off);
    let on = on_and_armed(Failover::After30m);
    assert!(!auto_promote_due(
        &id("m1"),
        &on,
        &off_grant,
        false,
        10 * HOUR
    ));
}

#[test]
fn unarmed_target_never_promotes() {
    let mut state = cluster();
    set(&mut state, Failover::After30m);
    let grantor = granted_at(1_000, Failover::After30m);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    arm(&mut state, "m1");
    assert!(auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    assert!(!auto_promote_due(
        &id("m2"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    let mut next = state.clone();
    apply(
        &mut next,
        Command::Term {
            term: Term {
                n: 2,
                node: id("m2"),
            },
        },
        500,
    )
    .expect("term");
    assert!(!auto_promote_due(
        &id("m1"),
        &next,
        &grantor,
        false,
        10 * HOUR
    ));
    let no_grant = Grantor::new(0);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &no_grant,
        false,
        10 * HOUR
    ));
    let mut revoked = granted_at(1_000, Failover::After30m);
    revoked.revoke();
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &revoked,
        false,
        10 * HOUR
    ));
}

#[test]
fn target_promotes_only_after_primary_fence() {
    for failover in [Failover::After30m, Failover::After1h] {
        let delay = failover.delay_ms().expect("delay");
        let drift = delay / 10_000 + 1;
        let state = on_and_armed(failover);
        for primary_rate in [-1_i64, 0, 1] {
            let primary_clock = |real: u64| -> u64 {
                let skew = i64::try_from(real / 10_000).expect("small") * primary_rate;
                u64::try_from(i64::try_from(real).expect("small") + skew).expect("positive")
            };
            let ask_real = 5 * MIN;
            let grant_real = ask_real + 700;
            let mut lease = PrimaryLease::new(0);
            let seq = lease.ask(&id("m1"), primary_clock(ask_real));
            lease.granted(&id("m1"), seq, failover, 0);
            let gates = lease.gates_until(&held());
            let mut gates_end_real = ask_real;
            while primary_clock(gates_end_real) < gates {
                gates_end_real += 1;
            }
            let grantor = granted_at(grant_real, failover);
            let promote_real = grantor.safe_after().expect("deadline");
            assert!(auto_promote_due(
                &id("m1"),
                &state,
                &grantor,
                false,
                promote_real
            ));
            assert!(
                promote_real >= gates_end_real + FENCE_LEAD_MS - drift,
                "{failover:?} rate {primary_rate}: promote {promote_real} gates {gates_end_real}"
            );
            assert!(!auto_promote_due(
                &id("m1"),
                &state,
                &grantor,
                false,
                promote_real - 1
            ));
        }
    }
}

#[test]
fn rebooted_grantor_waits_a_full_delay() {
    let mut grantor = Grantor::new(7_000);
    assert_eq!(grantor.safe_after(), None);
    grantor.grant(9_000, Failover::After30m);
    assert_eq!(grantor.safe_after(), Some(9_000 + HALF_HOUR));
    let rebooted = Grantor {
        booted_at: 5_000_000,
        last_grant: Some(100),
        applied: Failover::After30m,
        revoked: false,
    };
    assert_eq!(rebooted.safe_after(), Some(5_000_000 + HALF_HOUR));
    let long = Grantor {
        applied: Failover::After1h,
        ..rebooted
    };
    assert_eq!(long.safe_after(), Some(5_000_000 + HOUR));
    let off = granted_at(9_000, Failover::Off);
    assert_eq!(off.safe_after(), None);
}

#[test]
fn grantor_grants_only_highest_term_home() {
    let grantor = Grantor::new(0);
    let term = Term {
        n: 3,
        node: id("m5"),
    };
    assert!(grantor.may_grant(&term, &id("m5"), &term));
    let higher = Term {
        n: 4,
        node: id("m2"),
    };
    assert!(!grantor.may_grant(&term, &id("m5"), &higher));
    assert!(grantor.may_grant(&higher, &id("m2"), &higher));
    assert!(!grantor.may_grant(&higher, &id("m5"), &higher));
    let lower = Term {
        n: 2,
        node: id("m5"),
    };
    assert!(!grantor.may_grant(&lower, &id("m5"), &term));
}

#[test]
fn revoked_never_grants() {
    let mut grantor = Grantor::new(0);
    let term = Term {
        n: 3,
        node: id("m5"),
    };
    grantor.revoke();
    assert!(!grantor.may_grant(&term, &id("m5"), &term));
}

#[test]
fn reachable_primary_means_relay() {
    let state = on_and_armed(Failover::After30m);
    let grantor = granted_at(1_000, Failover::After30m);
    assert_eq!(
        promote_safety(&id("m1"), &state, &grantor, true, 10 * HOUR),
        Safety::Relay
    );
    let off = cluster();
    assert_eq!(
        promote_safety(&id("m1"), &off, &Grantor::new(0), true, 0),
        Safety::Relay
    );
}

#[test]
fn unsafe_promote_needs_confirm() {
    let text = "m5 can't be reached. If it's still running, its sessions keep going until it hears of this. Work it does after its last snapshot is kept as a copy on m5.";
    let off = cluster();
    assert_eq!(
        promote_safety(&id("m1"), &off, &Grantor::new(0), false, 10 * HOUR),
        Safety::NeedsConfirm(text.to_string())
    );
    let state = on_and_armed(Failover::After30m);
    let grantor = granted_at(1_000, Failover::After30m);
    assert_eq!(
        promote_safety(&id("m1"), &state, &grantor, false, 1_000 + HALF_HOUR - 1),
        Safety::NeedsConfirm(text.to_string())
    );
    assert_eq!(
        promote_safety(&id("m1"), &state, &grantor, false, 1_000 + HALF_HOUR),
        Safety::Safe
    );
    assert!(matches!(
        promote_safety(&id("m2"), &state, &grantor, false, 10 * HOUR),
        Safety::NeedsConfirm(_)
    ));
}

#[test]
fn gates_follow_the_delay_the_target_reported() {
    let mut state = on_and_armed(Failover::After1h);
    let mut lease = PrimaryLease::new(0);
    let mut grantor = Grantor::new(0);
    let first = lease.ask(&id("m1"), 1_000);
    grantor.grant(1_200, Failover::After1h);
    lease.granted(&id("m1"), first, Failover::After1h, 0);
    assert_eq!(lease.gates_until(&held()), 1_000 + HOUR - FENCE_LEAD_MS);

    set(&mut state, Failover::After30m);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    let lost = lease.ask(&id("m1"), 31_000);
    assert!(lost > first);
    assert_eq!(lease.gates_until(&held()), 1_000 + HOUR - FENCE_LEAD_MS);

    grantor.grant(31_200, Failover::After30m);
    let heard = lease.ask(&id("m1"), 61_000);
    grantor.grant(61_200, Failover::After30m);
    lease.granted(&id("m1"), heard, Failover::After30m, 0);
    assert_eq!(
        lease.gates_until(&held()),
        61_000 + HALF_HOUR - FENCE_LEAD_MS
    );
    assert_eq!(lease.reported(&id("m1")), Some(state.failover));
    arm(&mut state, "m1");
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        61_200 + HALF_HOUR - 1
    ));
    assert!(auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        61_200 + HALF_HOUR
    ));
}

#[test]
fn turning_off_stops_the_target_at_once() {
    let mut state = on_and_armed(Failover::After30m);
    let mut grantor = Grantor::new(0);
    grantor.grant(1_000, Failover::After30m);
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 900);
    lease.granted(&id("m1"), seq, Failover::After30m, 0);
    assert!(auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        2 * HOUR
    ));

    set(&mut state, Failover::Off);
    assert_eq!(state.armed, None);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        2 * HOUR
    ));
    assert_eq!(
        lease.gates_until(&BTreeMap::new()),
        900 + HALF_HOUR - FENCE_LEAD_MS
    );

    grantor.grant(31_000, Failover::Off);
    let off = lease.ask(&id("m1"), 31_000);
    lease.granted(&id("m1"), off, Failover::Off, 0);
    assert_eq!(lease.gates_until(&BTreeMap::new()), u64::MAX);
    assert_eq!(grantor.safe_after(), None);
}

#[test]
fn turning_on_arms_only_after_a_grant() {
    let mut state = cluster();
    assert_eq!(state.failover, Failover::Off);
    let term = state.term.clone();
    assert_eq!(
        apply(
            &mut state,
            Command::Armed {
                node: id("m1"),
                term: term.clone()
            },
            320
        )
        .err(),
        Some(CommandError::BadTransition)
    );
    set(&mut state, Failover::After1h);
    assert_eq!(state.armed, None);
    let grantor = granted_at(1_000, Failover::After1h);
    assert!(!auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    let lease = PrimaryLease::new(0);
    assert_eq!(lease.gates_until(&state.holders), u64::MAX);
    arm(&mut state, "m1");
    assert_eq!(state.armed, Some((id("m1"), term)));
    assert!(auto_promote_due(
        &id("m1"),
        &state,
        &grantor,
        false,
        10 * HOUR
    ));
    assert_eq!(lease.gates_until(&state.holders), 0);
}

#[test]
fn arming_is_checked_and_idempotent() {
    let mut state = cluster();
    set(&mut state, Failover::After30m);
    let term = state.term.clone();
    let stale = Term {
        n: 0,
        node: id("m5"),
    };
    for (node, at) in [
        ("m1", stale),
        ("m5", term.clone()),
        ("w1", term.clone()),
        ("nobody", term.clone()),
    ] {
        let result = apply(
            &mut state,
            Command::Armed {
                node: id(node),
                term: at,
            },
            330,
        );
        assert!(result.is_err(), "{node}");
    }
    assert_eq!(state.armed, None);
    arm(&mut state, "m1");
    let seq = state.seq;
    let again = apply(
        &mut state,
        Command::Armed {
            node: id("m1"),
            term,
        },
        340,
    )
    .expect("again");
    assert!(again.entry.is_none());
    assert_eq!(state.seq, seq);
    set(&mut state, Failover::After30m);
    assert!(state.armed.is_some());
    set(&mut state, Failover::After1h);
    assert_eq!(state.armed, None);
}

#[test]
fn older_state_without_the_new_fields_still_loads() {
    let state = cluster();
    let mut value = serde_json::to_value(&state).expect("json");
    let object = value.as_object_mut().expect("object");
    object.remove("failover");
    object.remove("armed");
    let back: ClusterState = serde_json::from_value(value).expect("old state");
    assert_eq!(back.failover, Failover::Off);
    assert_eq!(back.armed, None);
    let command: Command =
        serde_json::from_str(r#"{"set_failover":{"failover":"30m"}}"#).expect("command");
    assert_eq!(
        command,
        Command::SetFailover {
            failover: Failover::After30m
        }
    );
}

#[test]
fn failover_and_arming_survive_the_cluster_db() {
    let tmp = TempDir::new("cluster-failover");
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
    let commands = vec![
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
            term: state.term.clone(),
        },
    ];
    for command in commands {
        let applied = apply(&mut state, command, 2).expect("apply");
        save(
            &dir,
            applied.entry.as_ref().expect("entry"),
            &applied.changes,
        )
        .expect("save");
    }
    assert_eq!(load(&dir).expect("load").expect("state"), state);
    let applied = apply(
        &mut state,
        Command::SetFailover {
            failover: Failover::Off,
        },
        3,
    )
    .expect("off");
    save(
        &dir,
        applied.entry.as_ref().expect("entry"),
        &applied.changes,
    )
    .expect("save");
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded.failover, Failover::Off);
    assert_eq!(loaded.armed, None);
    assert_eq!(loaded, state);
}
