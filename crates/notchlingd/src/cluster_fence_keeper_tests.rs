use crate::cluster_fence::{mark_moved_in, read_fence};
use crate::cluster_fence_keeper::write_at_start;
use crate::cluster_fence_keeper_kit::{
    claim, conversation, mains, node, world, Seed, World, BOOT, S1, S2,
};
use notchling_engine::cluster::failover::{Failover, FENCE_LEAD_MS};
use notchling_engine::cluster::fence::{decide, Fence, Gate};
use notchling_engine::cluster::fence_gates::SetView;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::Tool;
use std::collections::BTreeMap;

fn two() -> Seed {
    Seed {
        main: mains(&[(S1, Tool::Claude), (S2, Tool::Claude)]),
        ..Seed::default()
    }
}

fn on(holder: bool) -> Seed {
    Seed {
        failover: Failover::After30m,
        holder,
        ..two()
    }
}

fn set(name: &str, movable: &[(&str, bool)]) -> SetView {
    SetView {
        set: name.to_string(),
        movable: movable
            .iter()
            .map(|(id, yes)| (id.to_string(), *yes))
            .collect(),
    }
}

fn gates_of(w: &World) -> BTreeMap<String, Gate> {
    w.fence().sessions
}

#[test]
fn start_waits_for_first_lease_with_failover_on() {
    let w = world("start-on", on(true));
    write_at_start(&w.cluster, &node("m5")).expect("written");
    let fence = w.fence();
    assert_eq!(fence.valid_until, 0);
    assert_eq!(fence.sessions[S1], Gate::Lease);
    assert_eq!(fence.sessions[&conversation(S1)], Gate::Lease);
    assert_eq!(fence.sessions[S2], Gate::Lease);
    let boot = notchling_platform::clock::boot_id().unwrap_or_default();
    assert_eq!(
        decide(Some(&fence), S1, &boot, 1_000),
        Fence::Wait,
        "a Lease session waits for the first lease"
    );
}

#[test]
fn start_with_failover_off_or_no_holder_never_runs_out() {
    for seed in [two(), on(false)] {
        let w = world("start-off", seed);
        write_at_start(&w.cluster, &node("m5")).expect("written");
        assert_eq!(w.fence().valid_until, u64::MAX);
    }
}

#[test]
fn start_without_a_cluster_writes_nothing() {
    let w = world("start-none", two());
    let _ = std::fs::remove_file(w.cluster.join("cluster.db"));
    write_at_start(&w.cluster, &node("m5")).expect("nothing to do");
    assert!(read_fence(&w.cluster).is_none());
}

#[test]
fn failover_off_never_runs_out() {
    let w = world("off", two());
    w.keeper.tick();
    let fence = w.fence();
    assert_eq!(fence.valid_until, u64::MAX);
    assert_eq!(
        decide(
            Some(&fence),
            S1,
            notchling_platform::clock::boot_id()
                .unwrap_or_default()
                .as_str(),
            u64::MAX - 1
        ),
        Fence::Pass,
        "Free passes after any sleep"
    );
    assert_eq!(gates_of(&w)[S1], Gate::Free);
}

#[test]
fn a_holder_with_no_lease_yet_gates_every_lease_session_at_zero() {
    let w = world("holder", on(true));
    *w.feed.latest.lock().unwrap() = Some(set("s1", &[(S1, true), (S2, true)]));
    w.keeper.tick();
    assert_eq!(w.fence().valid_until, 0);
    assert_eq!(gates_of(&w)[S1], Gate::Lease);
}

#[test]
fn a_fed_lease_sets_the_expiry() {
    let w = world("fed", on(true));
    *w.feed.latest.lock().unwrap() = Some(set("s1", &[(S1, true), (S2, true)]));
    {
        let mut lease = w.keeper.lease();
        let seq = lease.ask(&node("m1"), 5_000);
        let seen = notchling_engine::cluster::persist::load(&w.cluster)
            .unwrap()
            .unwrap()
            .holders
            .get(&node("m1"))
            .cloned();
        assert!(lease.granted(&node("m1"), seq, Failover::After30m, seen));
    }
    w.keeper.tick();
    assert_eq!(w.fence().valid_until, 5_000 + 1_800_000 - FENCE_LEAD_MS);
}

#[test]
fn movable_since_acked_set_is_lease_others_free() {
    let w = world("acked", on(false));
    w.feed.acked.lock().unwrap().insert(node("m1"), "s2".into());
    *w.feed.latest.lock().unwrap() = Some(set("s1", &[(S1, true), (S2, false)]));
    w.keeper.tick();
    *w.feed.latest.lock().unwrap() = Some(set("s2", &[(S1, false), (S2, false)]));
    w.keeper.tick();
    *w.feed.latest.lock().unwrap() = Some(set("s3", &[(S1, false), (S2, true)]));
    w.keeper.tick();
    let gates = gates_of(&w);
    assert_eq!(gates[S1], Gate::Free, "movable only before the acked set");
    assert_eq!(
        gates[S2],
        Gate::Lease,
        "movable in a set after the acked one"
    );
    assert_eq!(gates[&conversation(S2)], Gate::Lease);
}

#[test]
fn no_ready_target_frees_every_session() {
    let w = world("no-target", on(false));
    w.feed.ready.lock().unwrap().clear();
    w.keeper.tick();
    assert_eq!(gates_of(&w)[S1], Gate::Free);
    assert_eq!(gates_of(&w)[S2], Gate::Free);
}

#[test]
fn a_target_with_no_ack_counts_every_retained_set() {
    let w = world("no-ack", on(false));
    *w.feed.latest.lock().unwrap() = Some(set("s1", &[(S1, true), (S2, false)]));
    w.keeper.tick();
    assert_eq!(gates_of(&w)[S1], Gate::Lease);
    assert_eq!(gates_of(&w)[S2], Gate::Free);
}

#[test]
fn not_primary_writes_no_lease_or_free() {
    let w = world(
        "standby",
        Seed {
            home: "m1",
            claims: vec![claim(S2, "m1", 1)],
            ..on(false)
        },
    );
    w.keeper.tick();
    let gates = gates_of(&w);
    assert!(
        gates.values().all(|g| matches!(g, Gate::Moved { .. })),
        "{gates:?}"
    );
    assert!(!gates.contains_key(S1));
}

#[test]
fn claimed_elsewhere_is_moved() {
    let w = world(
        "claimed",
        Seed {
            claims: vec![claim(S1, "m1", 1)],
            ..two()
        },
    );
    w.keeper.tick();
    let moved = Gate::Moved { to: "m1".into() };
    assert_eq!(w.gate(S1), Some(moved.clone()));
    assert_eq!(w.gate(&conversation(S1)), Some(moved));
    assert_eq!(w.gate(S2), Some(Gate::Free));
}

#[test]
fn waiting_wins_over_moved() {
    let w = world(
        "waiting",
        Seed {
            claims: vec![claim(S1, "m1", 1)],
            ..two()
        },
    );
    w.keeper.tick();
    w.feed
        .waiting
        .lock()
        .unwrap()
        .insert(S1.to_string(), "m1".to_string());
    w.keeper.tick();
    let wait = Gate::Waiting {
        for_node: "m1".into(),
    };
    assert_eq!(w.gate(S1), Some(wait.clone()));
    assert_eq!(w.gate(&conversation(S1)), Some(wait));
    w.feed.waiting.lock().unwrap().clear();
    w.keeper.tick();
    assert_eq!(
        w.gate(S1),
        Some(Gate::Moved { to: "m1".into() }),
        "a Waiting gate that is no longer computed is dropped; the claim moves it"
    );
}

#[test]
fn keeper_keeps_a_moves_moved_gate() {
    let w = world("keeps", two());
    let at = Term {
        n: 2,
        node: node("m1"),
    };
    mark_moved_in(&w.cluster, &w.me, at, &[S1.to_string()], "m1", Some("c1")).unwrap();
    for _ in 0..3 {
        w.keeper.tick();
    }
    assert_eq!(w.gate(S1), Some(Gate::Moved { to: "m1".into() }));
    assert_eq!(w.gate(S2), Some(Gate::Free));
}

#[test]
fn the_keeper_never_removes_a_moved_gate() {
    let w = world("never-removes", two());
    let at = Term {
        n: 2,
        node: node("m1"),
    };
    mark_moved_in(&w.cluster, &w.me, at, &[S1.to_string()], "m1", Some("c1")).unwrap();
    w.reseed(&Seed {
        claims: vec![claim(S1, "m5", 3)],
        ..two()
    });
    w.keeper.tick();
    assert_eq!(
        w.gate(S1),
        Some(Gate::Moved { to: "m1".into() }),
        "only a restart here lifts it"
    );
}

#[test]
fn stale_lease_and_free_entries_are_dropped() {
    let w = world("drops", two());
    w.keeper.tick();
    assert!(w.gate(S2).is_some());
    w.reseed(&Seed::default());
    w.keeper.tick();
    assert_eq!(w.gate(S2), None);
    assert_eq!(w.gate(S1), Some(Gate::Free));
}

#[test]
fn writes_only_on_change() {
    use std::os::unix::fs::MetadataExt;
    let w = world("on-change", two());
    w.keeper.tick();
    let first = std::fs::metadata(w.path()).unwrap().ino();
    w.keeper.tick();
    w.keeper.tick();
    assert_eq!(std::fs::metadata(w.path()).unwrap().ino(), first);
    w.feed
        .waiting
        .lock()
        .unwrap()
        .insert(S1.into(), "m1".into());
    w.keeper.tick();
    assert_ne!(std::fs::metadata(w.path()).unwrap().ino(), first);
}

#[test]
fn no_cluster_writes_nothing() {
    let w = world("no-cluster", two());
    let _ = std::fs::remove_file(w.cluster.join("cluster.db"));
    w.keeper.tick();
    assert!(!w.path().exists());
    assert!(w.notice("cluster-fence-write").is_none());
}

#[test]
fn the_file_carries_the_term_node_boot_and_cluster() {
    let w = world("stamps", two());
    w.keeper.tick();
    let fence = w.fence();
    assert_eq!(fence.node, w.me);
    assert_eq!(fence.term.n, 1);
    let boot = notchling_platform::clock::boot_id().unwrap_or_default();
    assert_eq!(fence.boot_id, boot);
    let text = std::fs::read_to_string(w.path()).unwrap();
    assert!(text.contains("\"cluster\":\"c1\""), "{text}");
    let _ = BOOT;
}

#[test]
fn fence_file_is_atomic_and_0600() {
    use std::os::unix::fs::PermissionsExt;
    let w = world("atomic", two());
    w.keeper.tick();
    let mode = std::fs::metadata(w.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let names: Vec<String> = std::fs::read_dir(&w.cluster)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("fence"))
        .collect();
    assert_eq!(names, vec!["fence.json".to_string()], "no temp file left");
}

#[test]
fn planted_link_is_refused() {
    let w = world("link", two());
    let target = w.cluster.join("elsewhere.txt");
    std::fs::write(&target, b"untouched").unwrap();
    std::os::unix::fs::symlink(&target, w.path()).unwrap();
    w.keeper.tick();
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    assert!(std::fs::symlink_metadata(w.path())
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(w.notice("cluster-fence-write").is_some());
}

#[test]
fn damaged_file_is_reported_not_replaced() {
    let w = world("damaged", two());
    std::fs::write(w.path(), b"garbage").unwrap();
    w.keeper.tick();
    w.keeper.tick();
    assert_eq!(std::fs::read(w.path()).unwrap(), b"garbage");
    let text = w.notice("cluster-fence-write").expect("a notice");
    assert!(text.contains("fence.json"), "{text}");
    let raised = w
        .inbox
        .items()
        .into_iter()
        .filter(|item| item.id == "cluster-fence-write")
        .count();
    assert_eq!(raised, 1);
}

#[test]
fn a_new_error_raises_a_new_notice_text() {
    let w = world("two-errors", two());
    std::fs::write(w.path(), b"garbage").unwrap();
    w.keeper.tick();
    let first = w.notice("cluster-fence-write").unwrap();
    std::fs::remove_file(w.path()).unwrap();
    std::fs::create_dir(w.path()).unwrap();
    w.keeper.tick();
    let second = w.notice("cluster-fence-write").unwrap();
    assert_ne!(first, second);
}
