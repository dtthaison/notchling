use super::secrets_scenario::{stage, Stage};
use super::{check, run_seed_with, run_world, LeakAt, Plant, SecretPlant, Violation};
use crate::cluster::id::Qualified;
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

#[test]
fn marker_values_never_leave_their_paths() {
    let mut total = super::Stats::default();
    let all = seeds();
    for seed in &all {
        let (world, result) = run_world(*seed, 300, Plant::default());
        if let Err(v) = result {
            panic!("seed {seed} broke {v:?}");
        }
        let s = world.secrets().stats();
        total.released += s.released;
        total.stored += s.stored;
        total.answered += s.answered;
        total.revoked += s.revoked;
        total.consumed += s.consumed;
        total.resynced += s.resynced;
    }
    if all.len() >= 50 {
        assert!(total.released > 0, "no release in {} seeds", all.len());
        assert!(total.stored > 0, "no grant stored");
        assert!(total.answered > 0, "no approval answered");
        assert!(total.revoked > 0, "no grant revoked");
        assert!(total.consumed > 0, "no child consumed a grant");
        assert!(total.resynced > 0, "no mirror rebuilt after a crash");
    }
}

#[test]
fn a_planted_leak_is_caught_on_each_path() {
    let paths = [
        (LeakAt::Log, "log"),
        (LeakAt::Journal, "journal"),
        (LeakAt::Publish, "publish"),
        (LeakAt::Mirror, "mirror"),
        (LeakAt::Argv, "argv"),
        (LeakAt::Run, "run:shell.run"),
        (LeakAt::Answer, "answer"),
    ];
    for (at, expected) in paths {
        let Stage { mut world, cast } = stage(21, SecretPlant::default());
        world.plant_secrets(leak_at(at));
        world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
        world.run_for(6_000);
        assert_eq!(leaked_path(check(&world)), expected, "{at:?}");
    }
    let caught = (0..30).any(|seed| {
        matches!(
            run_seed_with(seed, 150, only(leak_at(LeakAt::Log))),
            Err(f) if matches!(f.violation, Violation::SecretLeaked { .. })
        )
    });
    assert!(caught);
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
    world.plant_secrets(SecretPlant {
        misroute: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), "wire:grant.deliver");
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
}

#[test]
fn a_grant_on_the_wrong_node_is_caught_by_the_engines_own_record() {
    let Stage { mut world, cast } = stage(31, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(check(&world), Ok(()));
    world.plant_grant(&cast.w2, &job_id("k1"), "tok-a");
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOnWrongNode { .. })
    ));
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
    assert_eq!(world.secrets().child_env_count(), 2);
}

#[test]
fn a_child_only_consumes_a_grant_when_it_starts() {
    let Stage { mut world, cast } = stage(32, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(world.secrets().child_env_count(), 1);
    assert_eq!(world.secrets().stats().stored, 1);
    world.request_secret(&job_id("k1"), "tok-c");
    world.run_for(6_000);
    assert_eq!(world.secrets().child_env_count(), 2);
}

#[test]
fn a_child_echoing_its_value_is_caught() {
    let Stage { mut world, cast } = stage(33, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        child_echo: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    assert_eq!(leaked_path(check(&world)), "log");
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
}

#[test]
fn the_janitor_alone_clears_a_grant_by_31_seconds() {
    let job = job_id("k1");
    let Stage { mut world, cast } = stage(34, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    world.partition(&cast.w1, &cast.m5);
    world.end_job(&job);
    world.run_for(32_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_grant_kept_past_the_janitor_is_caught_at_the_boundary() {
    let job = job_id("k1");
    let Stage { mut world, cast } = stage(34, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a"]);
    world.run_for(6_000);
    world.end_job(&job);
    world.run_for(33_000);
    assert_eq!(check(&world), Ok(()));
    world.run_for(3_000);
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
fn a_worker_refuses_a_deliver_for_a_job_it_does_not_know() {
    let Stage { mut world, cast } = stage(35, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    let known = job_id("k1");
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let expires = world.now() as i64 + 60_000;
    world.inject_deliver(&cast.w1, "j-nobody", "tok-a", (value, expires), 0);
    world.inject_deliver(&cast.w2, &known, "tok-a", (value, expires), 0);
    world.run_for(5_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
    world.inject_deliver(&cast.w1, &known, "tok-a", (value, expires), 0);
    world.run_for(2_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_worker_waits_up_to_three_seconds_for_a_job_to_be_known() {
    let Stage { mut world, cast } = stage(36, SecretPlant::default());
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let expires = world.now() as i64 + 60_000;
    let key = "k1";
    world.inject_deliver(&cast.w1, &job_id(key), "tok-a", (value, expires), 0);
    world.run_for(1_500);
    world.dispatch_with_secrets(&cast.m5, key, &cast.w1, &[]);
    world.run_for(2_500);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn an_expired_deliver_is_refused() {
    let Stage { mut world, cast } = stage(37, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let past = world.now() as i64 - 1;
    world.inject_deliver(&cast.w1, &job_id("k1"), "tok-a", (value, past), 0);
    world.run_for(2_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn a_deliver_that_arrived_before_its_revoke_is_refused() {
    let Stage { mut world, cast } = stage(38, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    let job = job_id("k1");
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let expires = world.now() as i64 + 60_000;
    world.inject_revoke(&cast.w1, &job, Some(&["tok-a"]));
    world.run_for(1_000);
    world.inject_deliver(&cast.w1, &job, "tok-a", (value, expires), 0);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    world.inject_deliver(&cast.w1, &job, "tok-b", (value, expires), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
}

#[test]
fn a_worker_storing_a_release_older_than_its_own_term_is_caught() {
    let Stage { mut world, cast } = stage(39, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    let job = job_id("k1");
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let expires = world.now() as i64 + 60_000;
    let current = world.worker_highest_term(&cast.w1).expect("a term");
    let newer = crate::cluster::members::Term {
        n: current.n + 1,
        node: current.node.clone(),
    };
    world.set_release_term(&job, "tok-a", newer.clone());
    world.inject_deliver(&cast.w1, &job, "tok-a", (value, expires), 0);
    world.run_for(1_000);
    assert_eq!(check(&world), Ok(()), "a release at or above the held term");
    let older = crate::cluster::members::Term {
        n: current.n.saturating_sub(1),
        node: current.node.clone(),
    };
    assert!(older < current);
    world.set_release_term(&job, "tok-b", older);
    world.inject_deliver(&cast.w1, &job, "tok-b", (value, expires), 0);
    world.run_for(1_000);
    assert!(matches!(
        check(&world),
        Err(Violation::ReleasedOffPrimary { .. })
    ));
}

#[test]
fn a_deliver_from_anyone_but_the_vault_node_is_refused() {
    let Stage { mut world, cast } = stage(40, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    let job = job_id("k1");
    let value = "{\"TOK\":\"NL-MARKER-900\"}";
    let expires = world.now() as i64 + 60_000;
    world.inject_deliver_from(&cast.w2, &cast.w1, &job, "tok-a", (value, expires), 0);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn answer_twice_is_caught() {
    let Stage { mut world, cast } = stage(24, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    assert_eq!(items.len(), 1, "{items:?}");
    let owner = Qualified::parse(&items[0]).map(|q| q.node);
    assert_eq!(owner, Some(cast.m5.clone()));
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
fn an_ask_first_entry_released_without_an_allow_is_caught() {
    let Stage { mut world, cast } = stage(39, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        skip_approval: true,
        ..SecretPlant::default()
    });
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(6_000);
    assert!(matches!(
        check(&world),
        Err(Violation::ReleasedWithoutAllow { .. })
    ));
    let Stage { mut world, cast } = stage(39, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    world.answer_item(&cast.m1, &items[0], true);
    world.run_for(4_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
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
fn a_login_failure_only_runs_read_only_probes() {
    let Stage { mut world, cast } = stage(28, SecretPlant::default());
    world.login_failure(&cast.w1);
    world.run_for(2_000);
    assert_eq!(check(&world), Ok(()));
    let runs = world.secrets().login_runs();
    assert!(runs
        .iter()
        .any(|(n, argv)| *n == cast.w1 && argv.last().map(String::as_str) == Some("status")));
    assert!(!world.secrets().mirrored(&cast.m5).is_empty());
}

#[test]
fn a_planted_refresh_on_a_worker_is_caught() {
    let Stage { mut world, cast } = stage(28, SecretPlant::default());
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
fn a_crashed_mirror_is_rebuilt_by_a_sync() {
    let Stage { mut world, cast } = stage(40, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-b"]);
    world.run_for(3_000);
    assert_eq!(world.secrets().mirrored(&cast.m1).len(), 1);
    world.crash(&cast.m1);
    assert!(world.secrets().mirrored(&cast.m1).is_empty());
    world.run_for(2_000);
    world.restart(&cast.m1);
    world.run_for(15_000);
    assert_eq!(world.secrets().mirrored(&cast.m1).len(), 1);
    assert!(world.secrets().stats().resynced >= 1);
    assert_eq!(check(&world), Ok(()));
}
