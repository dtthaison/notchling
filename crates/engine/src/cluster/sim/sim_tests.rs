use super::{
    run_seed, run_seed_with, run_snap_seed, run_snap_world, run_world, snap_cast,
    stale_call_scenario, Plant, SnapPlant, Violation, World,
};
use crate::cluster::testkit::id;

fn seeds() -> Vec<u64> {
    if let Ok(one) = std::env::var("NOTCHLING_SIM_SEED") {
        if let Ok(seed) = one.parse() {
            return vec![seed];
        }
    }
    let n: u64 = std::env::var("NOTCHLING_SIM_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    (0..n).collect()
}

#[test]
fn same_seed_same_trace() {
    let (a, ra) = run_world(7, 60, Plant::default());
    let (b, rb) = run_world(7, 60, Plant::default());
    assert_eq!(ra, rb);
    assert_eq!(a.trace(), b.trace());
    assert!(a.trace().len() > 100);
    let (c, _) = run_world(8, 60, Plant::default());
    assert_ne!(a.trace(), c.trace());
}

#[test]
fn invariants_hold_over_seeds() {
    for seed in seeds() {
        if let Err(failure) = run_seed(seed, 300) {
            panic!("{failure}");
        }
    }
}

#[test]
fn a_planted_double_start_is_caught() {
    let plant = Plant {
        double_start: true,
        skip_term_check: false,
    };
    let caught = (0..20).any(|seed| {
        matches!(
            run_seed_with(seed, 80, plant),
            Err(f) if matches!(f.violation, Violation::JobStartedTwice { .. })
        )
    });
    assert!(caught);
}

#[test]
fn a_planted_stale_call_is_caught() {
    let (_, planted) = stale_call_scenario(true);
    assert!(
        matches!(planted, Err(Violation::StaleTermAccepted { .. })),
        "{planted:?}"
    );
    let (world, honest) = stale_call_scenario(false);
    assert_eq!(honest, Ok(()));
    assert!(world.trace().iter().any(|l| l.contains("stepped_down")));
    assert!(world
        .trace()
        .iter()
        .any(|l| l.contains("refused") || l.contains("err=stale_term")));
}

#[test]
fn suspended_node_clock_keeps_counting() {
    let mut world = World::new(3);
    let a = world.add_node("m5", true);
    let b = world.add_node("m1", true);
    world.create(&a);
    world.join(&b, true);
    world.run_for(2_000);
    let before = world.boot_ms(&b);
    let lines = world.trace().len();
    world.suspend(&b);
    world.run_for(10_000);
    let after = world.boot_ms(&b);
    assert!(after >= before + 9_990, "{before} {after}");
    let quiet = world.trace()[lines..]
        .iter()
        .filter(|l| l.contains(" m1 ") && !l.contains("suspend"))
        .count();
    assert_eq!(quiet, 0);
    world.resume(&b);
    world.run_for(1_000);
    assert!(world.trace()[lines..]
        .iter()
        .any(|l| l.contains(" m1 ") && l.contains("resume")));
}

#[test]
fn rebooted_node_gets_new_boot_id() {
    let mut world = World::new(4);
    let a = world.add_node("m5", true);
    world.create(&a);
    world.run_for(5_000);
    let before = world.boot_id(&a);
    let clock = world.boot_ms(&a);
    assert!(clock >= 5_000);
    world.crash(&a);
    world.run_for(1_000);
    world.restart(&a);
    assert_ne!(world.boot_id(&a), before);
    assert!(world.boot_ms(&a) < 100);
    assert!(world.core(&a).and_then(|c| c.state()).is_some());
    let _ = id("m5");
}

#[test]
fn a_crashed_home_comes_back_with_its_journal() {
    let mut world = World::new(5);
    let a = world.add_node("m5", true);
    let b = world.add_node("w1", false);
    world.create(&a);
    world.join(&b, false);
    world.run_for(3_000);
    let jobs_before = world
        .core(&a)
        .and_then(|c| c.state())
        .map(|s| s.members.len());
    world.crash(&a);
    world.run_for(2_000);
    world.restart(&a);
    world.run_for(5_000);
    let jobs_after = world
        .core(&a)
        .and_then(|c| c.state())
        .map(|s| s.members.len());
    assert_eq!(jobs_before, jobs_after);
    assert!(world.core(&a).is_some_and(|c| c.online().contains(&b)));
}

fn finish_a_turn(world: &mut World, session: &str) {
    world.snap_session_work(session);
    world.run_for(3_000);
    world.snap_session_finish(session);
}

fn newest(world: &World, home: &crate::cluster::id::NodeId) -> Option<String> {
    world.snap_staged(home).last().cloned()
}

#[test]
fn standby_never_ahead_over_seeds() {
    for seed in seeds() {
        if let Err(failure) = run_snap_seed(seed, 150, SnapPlant::default()) {
            panic!("{failure}");
        }
    }
}

#[test]
fn snapshots_happen_in_the_sim() {
    let mut lines: Vec<String> = Vec::new();
    for seed in 0..40 {
        let (world, result) = run_snap_world(seed, 150, SnapPlant::default());
        assert_eq!(result, Ok(()));
        lines.extend(world.trace().iter().cloned());
    }
    for want in ["snapshot s-", "TurnEnd", "snap commit", "snap cut"] {
        assert!(lines.iter().any(|l| l.contains(want)), "{want}");
    }
}

#[test]
fn crash_during_transfer_keeps_last_complete_set() {
    let mut world = World::new(21);
    let cast = snap_cast(&mut world, SnapPlant::default());
    finish_a_turn(&mut world, "sim-a");
    world.run_for(60_000);
    let first = newest(&world, &cast.m5).expect("a first set");
    assert_eq!(world.snap_current(&cast.m1, &cast.m5), Some(first.clone()));
    finish_a_turn(&mut world, "sim-b");
    let mut waited = 0;
    while !world.snap_in_flight(&cast.m5, &cast.m1) && waited < 120_000 {
        world.run_for(50);
        waited += 50;
    }
    assert!(
        world.snap_in_flight(&cast.m5, &cast.m1),
        "no second transfer"
    );
    let second = newest(&world, &cast.m5).expect("a second set");
    assert_ne!(first, second);
    world.crash(&cast.m1);
    world.run_for(5_000);
    assert_eq!(world.snap_current(&cast.m1, &cast.m5), Some(first));
    assert_eq!(check_ok(&world), Ok(()));
    world.restart(&cast.m1);
    world.run_for(90_000);
    assert_eq!(
        world.snap_current(&cast.m1, &cast.m5),
        newest(&world, &cast.m5)
    );
    assert_eq!(check_ok(&world), Ok(()));
}

fn check_ok(world: &World) -> Result<(), Violation> {
    super::check(world)
}

#[test]
fn each_target_keeps_its_own_set() {
    let mut world = World::new(22);
    let cast = snap_cast(&mut world, SnapPlant::default());
    finish_a_turn(&mut world, "sim-a");
    world.run_for(60_000);
    let first = newest(&world, &cast.m5).expect("a first set");
    assert_eq!(world.snap_current(&cast.m1, &cast.m5), Some(first.clone()));
    assert_eq!(world.snap_current(&cast.m2, &cast.m5), Some(first.clone()));
    world.partition(&cast.m5, &cast.m2);
    finish_a_turn(&mut world, "sim-b");
    world.run_for(40_000);
    finish_a_turn(&mut world, "sim-c");
    world.run_for(40_000);
    let latest = newest(&world, &cast.m5).expect("a newer set");
    assert_ne!(first, latest);
    assert_eq!(world.snap_current(&cast.m1, &cast.m5), Some(latest.clone()));
    assert_eq!(world.snap_current(&cast.m2, &cast.m5), Some(first.clone()));
    let behind = world.snap_versions(&cast.m5, &first);
    let ahead = world.snap_versions(&cast.m5, &latest);
    assert!(ahead["sim-b"] > behind["sim-b"] && ahead["sim-c"] > behind["sim-c"]);
    assert_eq!(check_ok(&world), Ok(()));
    world.heal();
    world.run_for(120_000);
    let latest = newest(&world, &cast.m5).expect("a set");
    assert_eq!(world.snap_current(&cast.m2, &cast.m5), Some(latest.clone()));
    assert_eq!(world.snap_current(&cast.m1, &cast.m5), Some(latest));
    assert_eq!(check_ok(&world), Ok(()));
}

#[test]
fn a_planted_torn_commit_is_caught() {
    let plant = SnapPlant {
        torn_commit: true,
        ..SnapPlant::default()
    };
    let caught = (0..20).any(|seed| {
        matches!(
            run_snap_seed(seed, 80, plant),
            Err(f) if matches!(f.violation, Violation::TornSet { .. })
        )
    });
    assert!(caught);
    assert!((0..20).all(|seed| run_snap_seed(seed, 80, SnapPlant::default()).is_ok()));
}

#[test]
fn a_planted_unpushed_restore_is_caught() {
    let plant = SnapPlant {
        restore_unpushed: true,
        ..SnapPlant::default()
    };
    let caught = (0..40).any(|seed| {
        matches!(
            run_snap_seed(seed, 120, plant),
            Err(f) if matches!(f.violation, Violation::MovedUnpushed { .. })
        )
    });
    assert!(caught);
}
