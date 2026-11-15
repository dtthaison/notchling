use super::{move_cast, run_move_seed, run_move_world, World};

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
fn move_under_partitions_keeps_one_home_per_term() {
    for seed in seeds() {
        if let Err(failure) = run_move_seed(seed, 120) {
            panic!("{failure}");
        }
    }
}

#[test]
fn moves_happen_in_the_sim() {
    let mut lines: Vec<String> = Vec::new();
    for seed in 0..40 {
        let (world, result) = run_move_world(seed, 120);
        assert_eq!(result, Ok(()));
        lines.extend(world.trace().iter().cloned());
    }
    for want in ["call move.switch", "run move.switch", "stepped_down"] {
        assert!(lines.iter().any(|l| l.contains(want)), "{want}");
    }
}

#[test]
fn a_clean_move_hands_the_term_to_the_target() {
    let mut world = World::new(31);
    let cast = move_cast(&mut world);
    world.move_home(&cast.m5, &cast.m1);
    world.run_for(10_000);
    for node in [&cast.m5, &cast.m1, &cast.m2, &cast.w1] {
        let state = world.core(node).and_then(|c| c.state()).expect("state");
        assert_eq!(state.home, cast.m1, "{}", world.name_of(node));
        assert_eq!(state.term.n, 2);
    }
    assert!(world.thinks_home(&cast.m1));
    assert!(!world.thinks_home(&cast.m5));
}

#[test]
fn a_move_toward_a_cut_off_machine_changes_nothing() {
    let mut world = World::new(32);
    let cast = move_cast(&mut world);
    world.partition(&cast.m5, &cast.m2);
    world.run_for(30_000);
    world.move_home(&cast.m5, &cast.m2);
    world.run_for(30_000);
    assert!(world.thinks_home(&cast.m5));
    let state = world.core(&cast.m5).and_then(|c| c.state()).expect("state");
    assert_eq!(state.term.n, 1);
}
