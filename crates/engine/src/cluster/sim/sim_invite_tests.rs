use super::{check, Violation, World};

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

fn cast(seed: u64) -> (World, crate::cluster::NodeId, crate::cluster::NodeId) {
    let mut world = World::new(seed);
    let m5 = world.add_node("m5", true);
    let m1 = world.add_node("m1", true);
    let w1 = world.add_node("w1", false);
    let w2 = world.add_node("w2", false);
    let w3 = world.add_node("w3", false);
    world.create(&m5);
    world.join(&m1, true);
    world.join_invited(&w1, false, true);
    world.invite_join(&w3, &w2, (false, false));
    (world, w1, w3)
}

fn churn(world: &mut World, seed: u64) -> Result<(), String> {
    let everyone = world.node_ids();
    for step in 0..30 {
        let a = everyone[world.rng().below(everyone.len() as u64) as usize].clone();
        let b = everyone[world.rng().below(everyone.len() as u64) as usize].clone();
        match world.rng().below(10) {
            0 | 1 if a != b => world.partition(&a, &b),
            2 | 3 => world.heal(),
            4 => world.crash(&a),
            5 => world.restart(&a),
            6 => world.bump_term(&a),
            7 if a != b => world.invite_join(&a, &b, (false, false)),
            8 => world.invite_join(&a, &a, (true, true)),
            _ => {}
        }
        let settle = 200 + world.rng().below(2_000);
        world.run_for(settle);
        if let Err(v) = check(world) {
            return Err(format!("seed {seed} step {step}: {v:?}"));
        }
    }
    Ok(())
}

#[test]
fn no_member_active_without_approval_or_invite() {
    for seed in seeds() {
        let (mut world, w1, w3) = cast(seed);
        world.run_for(5_000);
        assert_eq!(check(&world), Ok(()), "seed {seed}");
        assert!(world.is_active_member(&w1), "seed {seed}: invited node");
        assert!(!world.is_active_member(&w3), "seed {seed}: wrong node");
        if let Err(text) = churn(&mut world, seed) {
            panic!("{text}");
        }
    }
}

#[test]
fn an_active_member_nobody_approved_is_caught() {
    let (mut world, w1, _) = cast(3);
    world.run_for(2_000);
    assert!(world.is_active_member(&w1));
    world.forget_approvals();
    assert!(matches!(
        check(&world),
        Err(Violation::ActiveWithoutApproval { .. })
    ));
}
