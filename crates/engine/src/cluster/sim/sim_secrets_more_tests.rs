use super::secrets_scenario::{stage, Stage};
use super::sim_secrets_tests::start;
use super::{check, run_world, Plant, SecretPlant, Violation, World};
use crate::cluster::id::{NodeId, Qualified, Tool};
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

const WINDOW: u64 = super::secrets::PENDING_MS;

#[test]
fn a_deliver_for_a_job_not_yet_started_is_held_pending_then_consumed_at_start() {
    let Stage { mut world, cast } = stage(35, SecretPlant::default());
    let at = world.now() as i64 + 3_600_000;
    let job = job_id("k1");
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert!(world.pending_on(&cast.w1, &job));
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    assert!(!world.pending_on(&cast.w1, &job));
    assert_eq!(world.secrets().child_env_count(), 1);
    world.run_for(WINDOW + 5_000);
    assert_eq!(
        world.secrets().grants_on(&cast.w1).len(),
        1,
        "live, not pending"
    );
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn an_attached_dispatch_reaches_the_worker_and_is_consumed_at_start() {
    let Stage { mut world, cast } = stage(41, SecretPlant::default());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &["tok-a", "tok-c"]);
    world.run_for(10_000);
    let stats = world.secrets().stats();
    assert_eq!(stats.released, 2);
    assert_eq!(stats.stored, 2);
    assert_eq!(stats.consumed, 2);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 2);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_claude_job_is_live_from_its_start_op_with_no_end_of_its_own() {
    let Stage { mut world, cast } = stage(54, SecretPlant::default());
    world.dispatch_tool(&cast.m5, "k1", (&cast.m1, Tool::Claude), &["tok-a"]);
    world.run_for(8_000);
    assert_eq!(world.secrets().stats().consumed, 1);
    world.end_job(&job_id("k1"));
    world.run_for(1_000);
    assert!(
        world.secrets().grants_on(&cast.m1).is_empty(),
        "the home's revoke removes it"
    );
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_pending_grant_is_swept_after_its_window() {
    let Stage { mut world, cast } = stage(55, SecretPlant::default());
    let job = job_id("k9");
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(WINDOW - 5_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    assert_eq!(check(&world), Ok(()));
    world.run_for(8_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_pending_grant_kept_past_its_window_is_caught_at_the_boundary() {
    let Stage { mut world, cast } = stage(56, SecretPlant::default());
    world.plant_secrets(SecretPlant {
        keep_grants: true,
        ..SecretPlant::default()
    });
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.w1, &job_id("k9"), "tok-a", (VALUE, at), 0);
    world.run_for(WINDOW + 1_000);
    assert_eq!(check(&world), Ok(()));
    world.run_for(2_000);
    assert!(matches!(
        check(&world),
        Err(Violation::GrantOutlivedJob { .. })
    ));
}

#[test]
fn a_deliver_at_the_age_of_the_window_starts_a_new_window() {
    let Stage { mut world, cast } = stage(57, SecretPlant::default());
    let job = job_id("k9");
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(WINDOW - 2_000);
    world.inject_deliver(&cast.w1, &job, "tok-b", (VALUE, at), 0);
    world.run_for(5_000);
    assert!(
        world.secrets().grants_on(&cast.w1).is_empty(),
        "the second deliver, inside the window, did not extend it"
    );
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(WINDOW - 5_000);
    world.inject_deliver(&cast.w1, &job, "tok-b", (VALUE, at), 0);
    world.run_for(WINDOW);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_whole_job_revoke_clears_pending_and_a_named_one_does_not() {
    let Stage { mut world, cast } = stage(58, SecretPlant::default());
    let job = job_id("k9");
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    world.run_for(200_000);
    world.inject_revoke(&cast.w1, &job, None);
    world.run_for(1_000);
    assert!(!world.pending_on(&cast.w1, &job));
    let seq = world.secrets().revoke_seq();
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), seq);
    world.run_for(WINDOW - 100_000);
    assert_eq!(
        world.secrets().grants_on(&cast.w1).len(),
        1,
        "a new window began with the later deliver"
    );
    world.inject_revoke(&cast.w1, &job, Some(&["tok-b"]));
    world.run_for(1_000);
    assert!(world.pending_on(&cast.w1, &job), "a named revoke leaves it");
    world.run_for(WINDOW);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_restart_between_deliver_and_start_sweeps_the_grant() {
    let Stage { mut world, cast } = stage(59, SecretPlant::default());
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.w1, &job_id("k1"), "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.w1).len(), 1);
    world.crash(&cast.w1);
    world.run_for(1_000);
    world.restart(&cast.w1);
    world.run_for(4_000);
    assert!(world.secrets().grants_on(&cast.w1).is_empty());
    world.dispatch_with_secrets(&cast.m5, "k1", &cast.w1, &[]);
    world.run_for(3_000);
    assert_eq!(world.secrets().child_env_count(), 0);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn the_state_wins_over_what_the_machine_saw_itself() {
    let Stage { mut world, cast } = stage(60, SecretPlant::default());
    world.dispatch_tool(&cast.m5, "k1", (&cast.m5, Tool::Claude), &[]);
    world.run_for(3_000);
    let job = job_id("k1");
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.m5, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.m5).len(), 1);
    world.drop_revokes(true);
    world.end_job(&job);
    world.run_for(3_000);
    assert!(
        world.secrets().grants_on(&cast.m5).is_empty(),
        "the state lists it finished, the start op says live"
    );
    let seq = world.secrets().revoke_seq();
    world.inject_deliver(&cast.m5, &job, "tok-b", (VALUE, at), seq);
    world.run_for(1_000);
    assert!(
        world.secrets().grants_on(&cast.m5).is_empty(),
        "a refused deliver"
    );
    world.inject_deliver(&cast.m5, &job_id("k2"), "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_deliver_for_a_job_the_state_lists_on_another_node_is_refused() {
    let Stage { mut world, cast } = stage(61, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.m5, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.m5).is_empty());
}

#[test]
fn a_failed_start_ends_the_job_and_refuses_later_delivers() {
    let Stage { mut world, cast } = stage(62, SecretPlant::default());
    let job = job_id("k1");
    let at = world.now() as i64 + 3_600_000;
    world.inject_deliver(&cast.m1, &job, "tok-a", (VALUE, at), 0);
    world.run_for(1_000);
    assert_eq!(world.secrets().grants_on(&cast.m1).len(), 1);
    world.fail_starts(true);
    world.dispatch_tool(&cast.m5, "k1", (&cast.m1, Tool::Claude), &[]);
    world.run_for(3_000);
    assert!(world.secrets().grants_on(&cast.m1).is_empty());
    let seq = world.secrets().revoke_seq();
    world.inject_deliver(&cast.m1, &job, "tok-b", (VALUE, at), seq);
    world.run_for(1_000);
    assert!(world.secrets().grants_on(&cast.m1).is_empty());
    assert_eq!(check(&world), Ok(()));
}

#[test]
fn a_deliver_for_a_job_that_ended_here_is_refused() {
    let Stage { mut world, cast } = stage(63, SecretPlant::default());
    let job = start(&mut world, &cast.m5, "k1", &cast.w1);
    world.end_job(&job);
    world.run_for(500);
    let at = world.now() as i64 + 3_600_000;
    let seq = world.secrets().revoke_seq();
    world.inject_deliver(&cast.w1, &job, "tok-a", (VALUE, at), seq);
    world.run_for(1_000);
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

#[test]
fn a_partitioned_stale_primary_still_releases_known_window() {
    let (world, result) = run_world(376, 300, Plant::default());
    assert_eq!(result, Ok(()));
    let s = world.secrets().stats();
    assert!(s.stale_releases >= 1, "{s:?}");
    assert!(s.split_accepts >= 1, "{s:?}");
    assert!(world
        .trace()
        .iter()
        .any(|l| l.contains("secrets Stats") && l.contains("split_accepts: 1")));
    let (world, result) = run_world(124, 300, Plant::default());
    assert_eq!(result, Ok(()));
    let s = world.secrets().stats();
    assert!(s.stale_releases >= 1 && s.split_accepts == 0, "{s:?}");
}
