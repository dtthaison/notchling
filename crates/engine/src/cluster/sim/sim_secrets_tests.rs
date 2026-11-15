use super::secrets_scenario::{stage, Stage};
use super::{check, run_seed_with, run_world, LeakAt, Plant, SecretPlant, Violation, World};
use crate::cluster::id::NodeId;
use crate::cluster::jobs::job_id;

pub(super) fn seeds() -> Vec<u64> {
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

fn leak_at(at: LeakAt) -> SecretPlant {
    SecretPlant {
        leak: Some(at),
        ..SecretPlant::default()
    }
}

fn leaked_path(result: Result<(), Violation>) -> String {
    match result {
        Err(Violation::SecretLeaked { path, .. }) => path,
        other => format!("not a leak: {other:?}"),
    }
}

/// Start a job with no secrets on a Worker: its own daemon then knows it.
pub(super) fn start(world: &mut World, from: &NodeId, key: &str, on: &NodeId) -> String {
    world.dispatch_with_secrets(from, key, on, &[]);
    world.run_for(3_000);
    job_id(key)
}

#[test]
fn marker_values_never_leave_their_paths() {
    let mut total = super::Stats::default();
    let all = seeds();
    for seed in &all {
        let (world, result) = run_world(*seed, 300, Plant::default());
        let s = world.secrets().stats();
        if let Err(v) = result {
            panic!("seed {seed} broke {v:?}; {s:?}");
        }
        total.released += s.released;
        total.stored += s.stored;
        total.answered += s.answered;
        total.revoked += s.revoked;
        total.consumed += s.consumed;
        total.resynced += s.resynced;
        total.stale_releases += s.stale_releases;
        total.split_accepts += s.split_accepts;
    }
    eprintln!("secrets over {} seeds: {total:?}", all.len());
    if all.len() >= 50 {
        assert!(total.released > 0, "no release in {} seeds", all.len());
        assert!(total.stored > 0, "no grant stored: {total:?}");
        assert!(total.answered > 0, "no approval answered");
        assert!(total.revoked > 0, "no grant revoked");
        assert!(total.consumed > 0, "no child consumed a grant");
        assert!(total.resynced > 0, "no mirror synced");
    }
}

fn planted_leak(at: LeakAt, expected: &str) {
    let Stage { mut world, cast } = stage(21, SecretPlant::default());
    world.plant_secrets(leak_at(at));
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), expected, "{at:?}");
}

#[test]
fn a_planted_log_leak_is_caught() {
    planted_leak(LeakAt::Log, "log");
}

#[test]
fn a_planted_journal_leak_is_caught() {
    planted_leak(LeakAt::Journal, "journal");
}

#[test]
fn a_planted_publish_leak_is_caught() {
    planted_leak(LeakAt::Publish, "publish");
}

#[test]
fn a_planted_mirror_leak_is_caught() {
    planted_leak(LeakAt::Mirror, "mirror");
}

#[test]
fn a_planted_inbox_leak_is_caught() {
    planted_leak(LeakAt::Inbox, "inbox");
}

#[test]
fn a_planted_argv_leak_is_caught() {
    planted_leak(LeakAt::Argv, "argv");
}

#[test]
fn a_planted_run_leak_is_caught() {
    planted_leak(LeakAt::Run, "run:session.reply");
}

#[test]
fn a_planted_answer_leak_is_caught() {
    planted_leak(LeakAt::Answer, "answer");
}

#[test]
fn a_planted_leak_is_caught_over_random_seeds() {
    let caught = (0..30).any(|seed| {
        matches!(
            run_seed_with(seed, 150, only(leak_at(LeakAt::Log))),
            Err(f) if matches!(f.violation, Violation::SecretLeaked { .. })
        )
    });
    assert!(caught);
}

#[test]
fn a_marker_in_a_local_only_inbox_item_is_caught() {
    let mut world = World::new(51);
    let solo = world.add_node("solo", true);
    world.create(&solo);
    world.run_for(2_000);
    assert!(world.mirror_targets(&solo).is_empty());
    assert_eq!(check(&world), Ok(()));
    world.plant_leak(&solo, LeakAt::Inbox, "NL-MARKER-77");
    assert_eq!(leaked_path(check(&world)), "inbox");
}

#[test]
fn a_leak_is_reported_before_any_other_violation() {
    let Stage { mut world, cast } = stage(30, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        misroute: true,
        worker_accepts_anything: true,
        refresh: true,
        ..SecretPlant::default()
    });
    world.login_failure(&cast.w2);
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), "wire:grant.deliver");
}

#[test]
fn a_secret_sent_to_the_wrong_node_leaks_on_the_wire() {
    let Stage { mut world, cast } = stage(29, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.plant_secrets(SecretPlant {
        misroute: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-a");
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), "wire:grant.deliver");
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
}

#[test]
fn a_grant_on_the_wrong_node_is_caught_by_the_engines_own_record() {
    let Stage { mut world, cast } = stage(31, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(6_000);
    assert_eq!(check(&world), Ok(()));
    world.plant_grant(&cast.w2, &job, "tok-a");
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOnWrongNode { .. })
    ));
}

#[test]
fn an_honest_grant_reaches_only_its_paths() {
    let Stage { mut world, cast } = stage(22, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(3_000);
    world.request_secret(&job, "tok-c");
    world.run_for(3_000);
    assert_eq!(check(&world), Ok(()));
    let held = world.secrets().grants_on(&cast.w1);
    assert_eq!(held.len(), 2, "{held:?}");
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
    assert!(world.secrets().grants_on(&cast.m5).is_empty());
    assert_eq!(world.secrets().child_env_count(), 2);
}

#[test]
fn an_attached_dispatch_to_a_worker_times_out_as_the_daemon_would() {
    let Stage { mut world, cast } = stage(41, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(10_000);
    assert!(world.secrets().stats().released >= 1);
    assert_eq!(world.secrets().stats().stored, 0);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_child_takes_only_what_the_response_delivered() {
    let Stage { mut world, cast } = stage(32, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    assert_eq!(world.secrets().child_env_count(), 1);
    let before = world.secrets().stats().stored;
    let refused = start(&mut world, &cast.m5, "k2", &cast.w2);
    world.request_secret(&refused, "tok-c");
    world.run_for(4_000);
    assert_eq!(world.secrets().child_env_count(), 1);
    assert_eq!(world.secrets().stats().stored, before);
}

#[test]
fn a_child_does_not_take_an_expired_grant_or_one_of_an_ended_job() {
    let Stage { mut world, cast } = stage(42, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let value = world.register_release(
        (&cast.m5, &job, &cast.w1),
        (
            world.worker_highest_term(&cast.m5).expect("a term"),
            world.worker_highest_term(&cast.m5).expect("a term"),
        ),
    );
    let expires = world.now() as i64 + 2_000;
    world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-a", (&value, expires), 0);
    world.run_for(500);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    world.run_for(2_500);
    world.use_grants(&cast.w1, &job, &["tok-a"]);
    assert_eq!(world.secrets().child_env_count(), 0, "expired");
    let again = world.register_release(
        (&cast.m5, &job, &cast.w1),
        (
            world.worker_highest_term(&cast.m5).expect("a term"),
            world.worker_highest_term(&cast.m5).expect("a term"),
        ),
    );
    let later = world.now() as i64 + 60_000;
    world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-b", (&again, later), 0);
    world.run_for(500);
    world.use_grants(&cast.w1, &job, &["tok-b"]);
    assert_eq!(world.secrets().child_env_count(), 1);
    world.end_job(&job);
    world.use_grants(&cast.w1, &job, &["tok-b"]);
    assert_eq!(world.secrets().child_env_count(), 1, "ended");
}

#[test]
fn a_child_echoing_its_value_is_caught() {
    let Stage { mut world, cast } = stage(33, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.plant_secrets(SecretPlant {
        child_echo: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-a");
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), "log");
}

#[test]
fn no_grant_after_job_end() {
    let Stage { mut world, cast } = stage(23, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    world.end_job(&job);
    world.run_for(40_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn the_janitor_alone_clears_a_grant_by_31_seconds() {
    let Stage { mut world, cast } = stage(34, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    world.partition(&cast.w1, &cast.m5);
    world.end_job(&job);
    world.run_for(31_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_grant_kept_past_the_janitor_is_caught_at_the_boundary() {
    let Stage { mut world, cast } = stage(34, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    world.end_job(&job);
    world.run_for(31_000);
    assert_eq!(check(&world), Ok(()));
    world.run_for(2_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOutlivedJob { .. })
    ));
}

#[test]
fn a_grant_with_no_record_of_its_job_still_has_a_deadline() {
    let Stage { mut world, cast } = stage(43, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    world.plant_orphan_grant(&cast.w1, "j-orphan", "tok-a");
    world.run_for(31_000);
    assert_eq!(check(&world), Ok(()));
    world.run_for(2_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOutlivedJob { .. })
    ));
}

#[test]
fn a_grant_kept_through_a_restart_is_caught_at_the_recovery_bound() {
    let Stage { mut world, cast } = stage(44, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    world.crash(&cast.w1);
    world.run_for(1_000);
    world.restart(&cast.w1);
    world.run_for(34_000);
    assert_eq!(check(&world), Ok(()));
    world.run_for(2_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOutlivedJob { .. })
    ));
}

#[test]
fn a_grant_in_flight_when_its_job_ends_is_gone_in_time() {
    for seed in 0..40 {
        let Stage { mut world, cast } = stage(seed, SecretPlant::default());
        let job = start(&mut world, &cast.m5, "k1", &cast.w1);
        world.request_secret(&job, "tok-a");
        world.request_secret(&job, "tok-c");
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
