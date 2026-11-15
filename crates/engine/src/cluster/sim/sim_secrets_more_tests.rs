use super::secrets_scenario::{stage, Stage};
use super::sim_secrets_tests::start;
use super::{check, run_world, Plant, SecretPlant, Violation, World};
use crate::cluster::id::{NodeId, Qualified};
use crate::cluster::jobs::job_id;
use crate::cluster::members::Term;

const VALUE: &str = "{\"TOK\":\"NL-MARKER-900\"}";

fn term(n: u64, node: &NodeId) -> Term {
    Term {
        n,
        node: node.clone(),
    }
}

fn expires(world: &World) -> i64 {
    world.now() as i64 + 60_000
}

#[test]
fn a_worker_refuses_a_deliver_for_a_job_it_does_not_know() {
    let Stage { mut world, cast } = stage(35, SecretPlant::default());
    let known = start(&mut world, &cast.m5, "k1", &cast.w1);
    let at = expires(&world);
    world.inject_deliver(&cast.w1, "j-nobody", "tok-a", (VALUE, at), 0);
    world.inject_deliver(&cast.w2, &known, "tok-a", (VALUE, at), 0);
    world.run_for(5_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert!(world.secrets().grants_on(&cast.w2).is_empty());
    world.inject_deliver(&cast.w1, &known, "tok-a", (VALUE, at), 0);
    world.run_for(2_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_worker_waits_up_to_three_seconds_for_a_job_to_be_known() {
    let Stage { mut world, cast } = stage(36, SecretPlant::default());
    let at = expires(&world);
    let key = "k1";
    world.inject_deliver(&cast.w1, &job_id(key), "tok-a", (VALUE, at), 0);
    world.run_for(1_500);
    world.dispatch_with_secrets(&cast.m5, key, &cast.w1, &[]);
    world.run_for(2_500);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_worker_gives_up_after_three_seconds() {
    let Stage { mut world, cast } = stage(46, SecretPlant::default());
    let at = expires(&world);
    world.inject_deliver(&cast.w1, &job_id("k1"), "tok-a", (VALUE, at), 0);
    world.run_for(3_800);
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(2_500);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn an_expired_deliver_is_refused() {
    let Stage { mut world, cast } = stage(37, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let past = world.now() as i64 - 1;
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, past), 0);
    world.run_for(2_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn a_deliver_that_arrived_before_its_revoke_is_refused() {
    let Stage { mut world, cast } = stage(38, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let at = expires(&world);
    world.inject_revoke(&cast.w1, &job, Some(&["tok-a"]));
    world.run_for(1_000);
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    world.inject_deliver(&cast.w1, &job, "tok-b", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
}

#[test]
fn a_whole_job_revoke_does_not_end_the_job() {
    let Stage { mut world, cast } = stage(47, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let at = expires(&world);
    world.inject_revoke(&cast.w1, &job, None);
    world.run_for(1_000);
    let seq = world.secrets().revoke_seq();
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), seq);
    world.run_for(1_000);
    assert_eq!(
        world.secrets().grants_on(&cast.w1).len(),
        1,
        "a fresh deliver"
    );
    world.inject_deliver(&cast.w1, &job, "tok-b", (VALUE, at), seq - 1);
    world.run_for(1_000);
    assert_eq!(
        world.secrets().grants_on(&cast.w1).len(),
        1,
        "one that raced it"
    );
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_deliver_from_anyone_but_the_vault_node_is_refused() {
    let Stage { mut world, cast } = stage(40, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let at = expires(&world);
    world.inject_deliver_from(&cast.w2, &cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
}

#[test]
fn a_worker_storing_a_release_older_than_its_own_term_is_caught() {
    let Stage { mut world, cast } = stage(39, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let held = world.worker_highest_term(&cast.w1).expect("a term");
    assert!(held.n >= 1);
    let at = expires(&world);
    let new = term(held.n + 1, &cast.m5);
    let value = world.register_release((&cast.m5, &job, &cast.w1), (new.clone(), new));
    world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-a", (&value, at), 0);
    world.run_for(1_000);
    assert_eq!(check(&world), Ok(()), "a release at or above the held term");
    let old = term(held.n - 1, &cast.m5);
    assert!(old < held);
    let value = world.register_release((&cast.m5, &job, &cast.w1), (old, held));
    world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-b", (&value, at), 0);
    world.run_for(1_000);
    assert!(matches!(
        check(&world),
        Err(Violation::ReleasedOffPrimary { .. })
    ));
}

#[test]
fn overlapping_releases_of_one_name_each_keep_their_own_term() {
    for low_first in [true, false] {
        let Stage { mut world, cast } = stage(48, SecretPlant::default());
        let job = start(&mut world, &cast.m5, "k1", &cast.w1);
        let held = world.worker_highest_term(&cast.w1).expect("a term");
        let (old, new) = (term(held.n - 1, &cast.m5), term(held.n + 1, &cast.m5));
        let low = world.register_release((&cast.m5, &job, &cast.w1), (old, new.clone()));
        let high = world.register_release((&cast.m5, &job, &cast.w1), (new.clone(), new));
        let at = expires(&world);
        let order = if low_first {
            [&low, &high]
        } else {
            [&high, &low]
        };
        for value in order {
            world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-a", (value, at), 0);
            world.run_for(600);
        }
        assert!(
            matches!(check(&world), Err(Violation::ReleasedOffPrimary { .. })),
            "low first: {low_first}"
        );
    }
    let Stage { mut world, cast } = stage(48, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let held = world.worker_highest_term(&cast.w1).expect("a term");
    let new = term(held.n + 1, &cast.m5);
    let high = world.register_release((&cast.m5, &job, &cast.w1), (new.clone(), new));
    let at = expires(&world);
    world.inject_deliver_from(&cast.m5, &cast.w1, &job, "tok-a", (&high, at), 0);
    world.run_for(600);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_secret_for_one_node_cannot_go_to_another_node_running_a_job_of_the_same_id() {
    let Stage { mut world, cast } = stage(49, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.know_job(&cast.w2, &job);
    let held = world.worker_highest_term(&cast.m5).expect("a term");
    let mine = world.register_release((&cast.m5, &job, &cast.w1), (held.clone(), held.clone()));
    let theirs = world.register_release((&cast.m5, &job, &cast.w2), (held.clone(), held));
    let at = expires(&world);
    world.inject_deliver_from(&cast.m5, &cast.w2, &job, "tok-a", (&theirs, at), 0);
    world.run_for(600);
    assert_eq!(check(&world), Ok(()), "each to the node it is for");
    world.inject_deliver_from(&cast.m5, &cast.w2, &job, "tok-b", (&mine, at), 0);
    world.run_for(600);
    assert!(
        matches!(check(&world), Err(Violation::SecretLeaked { .. })),
        "w1's secret reached w2"
    );
}

#[test]
fn answer_twice_is_caught() {
    let Stage { mut world, cast } = stage(24, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
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
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.plant_secrets(SecretPlant {
        answer_twice: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-b");
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
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
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
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.plant_secrets(SecretPlant {
        skip_approval: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-b");
    world.run_for(6_000);
    assert!(matches!(
        check(&world),
        Err(Violation::ReleasedWithoutAllow { .. })
    ));
    let Stage { mut world, cast } = stage(39, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    world.answer_item(&cast.m1, &items[0], true);
    world.run_for(4_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
}

#[test]
fn one_allow_once_does_not_cover_a_later_release() {
    let Stage { mut world, cast } = stage(52, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
    world.run_for(3_000);
    let items = world.secrets().mirrored(&cast.m1);
    world.answer_item(&cast.m1, &items[0], true);
    world.run_for(4_000);
    assert_eq!(check(&world), Ok(()));
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    world.inject_revoke(&cast.w1, &job, Some(&["tok-b"]));
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    world.plant_secrets(SecretPlant {
        skip_approval: true,
        ..SecretPlant::default()
    });
    world.request_secret(&job, "tok-b");
    world.run_for(4_000);
    assert!(matches!(
        check(&world),
        Err(Violation::ReleasedWithoutAllow { .. })
    ));
}

#[test]
fn a_planted_release_off_primary_is_caught() {
    for plant in [false, true] {
        let Stage { mut world, cast } = stage(26, SecretPlant::default());
        let job = start(&mut world, &cast.m5, "k1", &cast.w1);
        world.request_secret(&job, "tok-a");
        world.run_for(4_000);
        world.plant_secrets(SecretPlant {
            release_off_primary: plant,
            ..SecretPlant::default()
        });
        world.bump_term(&cast.m1);
        world.run_for(4_000);
        world.request_secret(&job, "tok-c");
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
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-a");
    world.run_for(4_000);
    world.request_secret(&job, "tok-c");
    world.run_for(4_000);
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
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
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

#[test]
fn a_mirror_that_went_missing_is_resynced_within_a_minute() {
    let Stage { mut world, cast } = stage(53, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.request_secret(&job, "tok-b");
    world.run_for(3_000);
    assert_eq!(world.secrets().mirrored(&cast.m1).len(), 1);
    world.forget_mirror(&cast.m1);
    assert!(world.secrets().mirrored(&cast.m1).is_empty());
    world.run_for(500);
    assert!(world.secrets().mirrored(&cast.m1).is_empty());
    world.run_for(61_000);
    assert_eq!(world.secrets().mirrored(&cast.m1).len(), 1);
}

/// Seed 1372 is an honest run in which a primary released under a term
/// that is no longer the newest, and a Worker stored that release while a
/// newer term existed elsewhere (a split-brain window). The check passes it;
/// the window is counted. Seed 62 releases off the newest term with no accept.
#[test]
fn a_partitioned_stale_primary_still_releases_known_window() {
    let (world, result) = run_world(1_372, 300, Plant::default());
    assert_eq!(result, Ok(()));
    let s = world.secrets().stats();
    assert!(s.stale_releases >= 1, "{s:?}");
    assert!(s.split_accepts >= 1, "{s:?}");
    assert!(world
        .trace()
        .iter()
        .any(|l| l.contains("secrets Stats") && l.contains("split_accepts: 1")));
    let (world, result) = run_world(62, 300, Plant::default());
    assert_eq!(result, Ok(()));
    let s = world.secrets().stats();
    assert!(s.stale_releases >= 1 && s.split_accepts == 0, "{s:?}");
}
