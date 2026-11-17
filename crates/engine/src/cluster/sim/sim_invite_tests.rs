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

fn forged_invite_joined(node: &crate::cluster::NodeId) -> crate::cluster::core::Effect {
    crate::cluster::core::Effect::InviteJoined {
        node: node.clone(),
        name: "forged".to_string(),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        minter: node.clone(),
        minter_name: "forged".to_string(),
    }
}

#[test]
fn an_invite_joined_for_a_node_nobody_invited_is_not_approval() {
    let mut world = World::new(5);
    let m5 = world.add_node("m5", true);
    let w1 = world.add_node("w1", false);
    world.create(&m5);
    world.join(&w1, false);
    world.run_for(2_000);
    assert!(world.is_active_member(&w1));
    world.forget_approvals();
    world
        .secrets
        .planted
        .push((m5.clone(), forged_invite_joined(&w1)));
    world.run_for(2_000);
    assert!(!world.is_approved(&w1));
    assert!(matches!(
        check(&world),
        Err(Violation::ActiveWithoutApproval { .. })
    ));
}

#[test]
fn an_invite_joined_for_an_invited_node_is_approval() {
    let mut world = World::new(5);
    let m5 = world.add_node("m5", true);
    let w1 = world.add_node("w1", false);
    world.create(&m5);
    world.join(&w1, false);
    world.run_for(2_000);
    world.forget_approvals();
    world.local(
        &m5,
        crate::cluster::core::LocalCmd::Invite {
            secret: "s".to_string().into(),
            invite: crate::cluster::invite::Invite {
                node: w1.clone(),
                name: "w1".to_string(),
                can_be_master: false,
                acting: false,
            },
            minter: m5.clone(),
        },
    );
    world
        .secrets
        .planted
        .push((m5.clone(), forged_invite_joined(&w1)));
    world.run_for(2_000);
    assert!(world.is_approved(&w1));
}
