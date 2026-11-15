use super::secrets_scenario::{stage, Stage};
use super::{check, run_seed_with, run_world, Plant, SecretPlant, Violation};
use crate::cluster::jobs::job_id;

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

fn only(plant: SecretPlant) -> Plant {
    Plant {
        secrets: plant,
        ..Plant::default()
    }
}

#[test]
fn marker_values_never_leave_their_paths() {
    let mut released = 0;
    let mut stored = 0;
    let mut answered = 0;
    let mut revoked = 0;
    let all = seeds();
    for seed in &all {
        let (world, result) = run_world(*seed, 300, Plant::default());
        if let Err(v) = result {
            panic!("seed {seed} broke {v:?}");
        }
        let stats = world.secrets().stats();
        released += stats.released;
        stored += stats.stored;
        answered += stats.answered;
        revoked += stats.revoked;
    }
    if all.len() >= 50 {
        assert!(released > 0, "no release in {} seeds", all.len());
        assert!(stored > 0, "no grant stored");
        assert!(answered > 0, "no approval answered");
        assert!(revoked > 0, "no grant revoked");
    }
}

#[test]
fn a_planted_leak_is_caught() {
    let Stage { mut world, cast } = stage(21, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        leak: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert!(matches!(check(&world), Err(Violation::SecretLeaked { .. })));
    let leak = SecretPlant {
        leak: true,
        ..SecretPlant::default()
    };
    let caught = (0..30).any(|seed| {
        matches!(
            run_seed_with(seed, 150, only(leak)),
            Err(f) if matches!(f.violation, Violation::SecretLeaked { .. })
        )
    });
    assert!(caught);
}

#[test]
fn an_honest_grant_reaches_only_its_paths() {
    let Stage { mut world, cast } = stage(22, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a", "tok-c"]);
    world.run_for(6_000);
    assert_eq!(check(&world), Ok(()));
    let held = world.secrets().grants_on(&cast.w1);
    assert_eq!(held.len(), 2, "{held:?}");
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
    assert!(world.secrets().grants_on(&cast.m5).is_empty());
    assert!(world.secrets().child_env_count() >= 2);
}

#[test]
fn no_grant_after_job_end() {
    let job = job_id("k1");
    let Stage { mut world, cast } = stage(23, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    world.end_job(&job);
    world.run_for(40_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));

    let Stage { mut world, cast } = stage(23, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    world.end_job(&job);
    world.run_for(120_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOutlivedJob { .. })
    ));
}

#[test]
fn a_grant_in_flight_when_its_job_ends_is_gone_in_time() {
    for seed in 0..40 {
        let job = job_id("k1");
        let Stage { mut world, cast } = stage(seed, SecretPlant::default());
        world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a", "tok-c"]);
        world.run_for(5 + seed % 40);
        world.end_job(&job);
        world.run_for(90_000);
        assert_eq!(check(&world), Ok(()), "seed {seed}");
        assert!(
            world.secrets().grants_on(&cast.w1).is_empty(),
            "seed {seed}"
        );
    }
}

#[test]
fn answer_twice_is_caught() {
    let Stage { mut world, cast } = stage(24, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    assert_eq!(items.len(), 1, "{items:?}");
    world.answer_item(&cast.m5, &items[0], true);
    world.answer_item(&cast.m1, &items[0], true);
    world.run_for(4_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().answered(&items[0]), 1);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);

    let Stage { mut world, cast } = stage(24, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        answer_twice: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    world.answer_item(&cast.m5, &items[0], true);
    world.answer_item(&cast.m1, &items[0], true);
    world.run_for(4_000);
    assert!(matches!(check(&world), Err(Violation::AnswerTwice { .. })));
}

#[test]
fn a_denied_approval_releases_nothing() {
    let Stage { mut world, cast } = stage(25, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    world.answer_item(&cast.m1, &items[0], false);
    world.run_for(4_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().stats().released, 0);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn a_planted_release_off_primary_is_caught() {
    for plant in [false, true] {
        let Stage { mut world, cast } = stage(26, SecretPlant::default());
        world.plant_secrets(SecretPlant {
            release_off_primary: plant,
            ..SecretPlant::default()
        });
        world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
        world.run_for(6_000);
        world.bump_term(&cast.m1);
        world.run_for(4_000);
        world.request_secret(&job_id("k1"), "tok-c");
        world.run_for(6_000);
        let result = check(&world);
        if plant {
            assert!(
                matches!(result, Err(Violation::ReleasedOffPrimary { .. })),
                "{result:?}"
            );
        } else {
            assert_eq!(result, Ok(()));
            assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
        }
    }
}

#[test]
fn a_request_on_the_primary_vault_is_granted() {
    let Stage { mut world, cast } = stage(27, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    world.request_secret(&job_id("k1"), "tok-c");
    world.run_for(6_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 2);
}

#[test]
fn a_planted_refresh_on_a_worker_is_caught() {
    let Stage { mut world, cast } = stage(28, SecretPlant::default());
    world.login_failure(&cast.w1);
    world.run_for(2_000);
    assert_eq!(check(&world), Ok(()));
    assert!(!world.secrets().mirrored(&cast.m5).is_empty());
    world.plant_secrets(SecretPlant {
        refresh: true,
        ..SecretPlant::default()
    });
    world.login_failure(&cast.w2);
    world.run_for(2_000);
    assert!(matches!(
        check(&world),
        Err(Violation::RefreshOnWorker { .. })
    ));
}

#[test]
fn a_planted_misdelivery_is_caught() {
    let Stage { mut world, cast } = stage(29, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        misdeliver: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOnWrongNode { .. })
    ));
}
