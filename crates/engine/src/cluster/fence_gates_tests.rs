use std::collections::BTreeMap;

use super::claims::Claim;
use super::failover::{Failover, PrimaryLease, FENCE_LEAD_MS};
use super::failover_tests::{cluster, on_and_armed};
use super::fence::Gate;
use super::fence_gates::{gates, lease_target, may_move, valid_until, LocalSession, SetView};
use super::id::NodeId;
use super::members::Term;
use super::state::ClusterState;
use super::testkit::id;

fn names(node: &NodeId) -> String {
    if *node == id("m1") {
        "m1".to_string()
    } else if *node == id("m5") {
        "m5".to_string()
    } else {
        "elsewhere".to_string()
    }
}

fn session(id: &str, lease: bool) -> LocalSession {
    LocalSession {
        id: id.to_string(),
        keys: vec![id.to_string(), format!("conv-{id}")],
        lease,
    }
}

fn claim(session: &str, node: &str) -> Claim {
    Claim {
        session: session.to_string(),
        node: id(node),
        n: 1,
        term: Term {
            n: 2,
            node: id(node),
        },
        base_set: "set".to_string(),
        base_len: 10,
        touched: false,
    }
}

fn with_claim(mut state: ClusterState, session: &str, node: &str) -> ClusterState {
    state
        .claims
        .insert(session.to_string(), claim(session, node));
    state
}

fn none() -> BTreeMap<String, String> {
    BTreeMap::new()
}

#[test]
fn the_primary_leases_movable_sessions_and_frees_the_rest() {
    let state = cluster();
    let out = gates(
        &id("m5"),
        &state,
        &[session("a", true), session("b", false)],
        &none(),
        &names,
    );
    assert_eq!(out["a"], Gate::Lease);
    assert_eq!(out["conv-a"], Gate::Lease);
    assert_eq!(out["b"], Gate::Free);
    assert_eq!(out["conv-b"], Gate::Free);
    assert_eq!(out.len(), 4);
}

#[test]
fn not_primary_writes_no_lease_or_free() {
    let state = cluster();
    let out = gates(
        &id("m1"),
        &state,
        &[session("a", true), session("b", false)],
        &none(),
        &names,
    );
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn claimed_elsewhere_is_moved_with_every_key() {
    let state = with_claim(cluster(), "a", "m1");
    let out = gates(
        &id("m5"),
        &state,
        &[session("a", true), session("b", true)],
        &none(),
        &names,
    );
    let moved = Gate::Moved { to: "m1".into() };
    assert_eq!(out["a"], moved);
    assert_eq!(out["conv-a"], moved);
    assert_eq!(out["b"], Gate::Lease);
}

#[test]
fn a_claim_naming_this_node_moves_nothing() {
    let state = with_claim(cluster(), "a", "m5");
    let out = gates(&id("m5"), &state, &[session("a", false)], &none(), &names);
    assert_eq!(out["a"], Gate::Free);
}

#[test]
fn a_claim_without_a_local_session_gates_its_id_alone() {
    let state = with_claim(cluster(), "gone", "m1");
    let out = gates(&id("m5"), &state, &[], &none(), &names);
    assert_eq!(out.len(), 1);
    assert_eq!(out["gone"], Gate::Moved { to: "m1".into() });
}

#[test]
fn a_claim_by_an_unnamed_machine_is_named_by_the_callback() {
    let state = with_claim(cluster(), "a", "m2");
    let out = gates(&id("m5"), &state, &[session("a", true)], &none(), &names);
    assert_eq!(
        out["a"],
        Gate::Moved {
            to: "elsewhere".into()
        }
    );
}

#[test]
fn waiting_wins_over_moved_and_lease() {
    let state = with_claim(cluster(), "a", "m1");
    let waiting = BTreeMap::from([
        ("a".to_string(), "m1".to_string()),
        ("b".to_string(), "m1".to_string()),
    ]);
    let out = gates(
        &id("m5"),
        &state,
        &[session("a", true), session("b", true)],
        &waiting,
        &names,
    );
    let wait = Gate::Waiting {
        for_node: "m1".into(),
    };
    assert_eq!(out["a"], wait);
    assert_eq!(out["conv-a"], wait);
    assert_eq!(out["b"], wait);
    assert_eq!(out["conv-b"], wait);
}

#[test]
fn moved_wins_over_lease_and_free_on_the_primary() {
    let state = with_claim(cluster(), "a", "m1");
    let out = gates(&id("m5"), &state, &[session("a", false)], &none(), &names);
    assert_eq!(out["a"], Gate::Moved { to: "m1".into() });
}

#[test]
fn a_waiting_session_without_local_keys_gates_its_id_alone() {
    let waiting = BTreeMap::from([("x".to_string(), "m1".to_string())]);
    let out = gates(&id("m1"), &cluster(), &[], &waiting, &names);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out["x"],
        Gate::Waiting {
            for_node: "m1".into()
        }
    );
}

#[test]
fn failover_off_never_runs_out() {
    let state = cluster();
    assert_eq!(state.failover, Failover::Off);
    assert_eq!(valid_until(&state, &PrimaryLease::new(0)), u64::MAX);
    let mut state = on_and_armed(Failover::After30m);
    state.failover = Failover::Off;
    assert_eq!(valid_until(&state, &PrimaryLease::new(0)), u64::MAX);
}

#[test]
fn failover_on_with_no_holder_never_runs_out() {
    let mut state = cluster();
    state.failover = Failover::After1h;
    assert!(state.holders.is_empty());
    assert_eq!(valid_until(&state, &PrimaryLease::new(0)), u64::MAX);
}

#[test]
fn a_holder_with_no_lease_yet_gates_at_zero() {
    let state = on_and_armed(Failover::After30m);
    assert!(!state.holders.is_empty());
    assert_eq!(valid_until(&state, &PrimaryLease::new(0)), 0);
}

#[test]
fn a_granted_lease_gates_before_the_deadline() {
    let state = on_and_armed(Failover::After30m);
    let mut lease = PrimaryLease::new(0);
    let seq = lease.ask(&id("m1"), 1_000);
    assert!(lease.granted(
        &id("m1"),
        seq,
        Failover::After30m,
        state.holders.get(&id("m1")).cloned()
    ));
    assert_eq!(
        valid_until(&state, &lease),
        1_000 + 1_800_000 - FENCE_LEAD_MS
    );
}

#[test]
fn the_lease_target_exists_only_with_failover_on() {
    let mut state = cluster();
    assert_eq!(lease_target(&state, &|_| true), None);
    state.failover = Failover::After30m;
    assert_eq!(lease_target(&state, &|_| true), Some(id("m1")));
    assert_eq!(lease_target(&state, &|n| *n != id("m1")), Some(id("m2")));
    assert_eq!(lease_target(&state, &|_| false), None);
}

fn view(set: &str, entries: &[(&str, bool)]) -> SetView {
    SetView {
        set: set.to_string(),
        movable: entries.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

#[test]
fn movable_since_acked_set_is_lease_others_free() {
    let history = [
        view("s1", &[("a", true), ("b", false), ("c", false)]),
        view("s2", &[("a", false), ("b", false), ("c", true)]),
        view("s3", &[("a", false), ("b", false), ("c", false)]),
    ];
    assert!(may_move(&history, Some("s1"), "a"));
    assert!(!may_move(&history, Some("s2"), "a"));
    assert!(may_move(&history, Some("s2"), "c"));
    assert!(!may_move(&history, Some("s3"), "c"));
    assert!(!may_move(&history, Some("s1"), "b"));
}

#[test]
fn no_ack_yet_counts_every_retained_set() {
    let history = [
        view("s1", &[("a", true), ("b", false)]),
        view("s2", &[("a", false), ("b", false)]),
    ];
    assert!(may_move(&history, None, "a"));
    assert!(!may_move(&history, None, "b"));
}

#[test]
fn doubt_counts_as_lease() {
    let history = [view("s1", &[("a", false)])];
    assert!(may_move(&history, Some("unknown-set"), "a"));
    assert!(may_move(&[], None, "a"));
    assert!(may_move(&[], Some("s1"), "a"));
    assert!(may_move(&history, Some("s1"), "never-listed"));
}
