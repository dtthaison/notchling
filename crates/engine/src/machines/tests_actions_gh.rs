//! Ported from `MachinesMorningTests.swift`: the actions that write to GitHub through `gh`, and
//! the registration checks every action makes first.

use std::sync::Arc;

use super::actions::{MergeFunction, MorningActions};
use super::fakes_jobs::{bad_code, AnswerRunner, Counter};
use super::merge::MergeOutcome;
use super::morning::{MorningItem, MorningKind};
use super::runner::CommandResult;
use super::tests_actions::{actions, item, item_of, m1, plan, silent, REPO};

// gh actions

#[test]
fn send_back_labels_and_prefix() {
    let runner = silent();
    actions(&runner).send_back(&item(MorningKind::Ready, 7), "tighten the spacing");
    assert_eq!(
        runner.gh_calls(),
        [
            vec![
                "issue",
                "comment",
                "7",
                "-R",
                REPO,
                "--body",
                "notchling: tighten the spacing"
            ],
            vec![
                "issue",
                "edit",
                "7",
                "-R",
                REPO,
                "--add-label",
                "sd:queued",
                "--remove-label",
                "sd:ready"
            ],
        ]
    );
}

#[test]
fn failed_comment_leaves_the_labels() {
    let runner = Arc::new(AnswerRunner::new(|_| {
        CommandResult::new(1, "", "rate limited")
    }));
    let a = actions(&runner);
    let it = item(MorningKind::Ready, 7);
    a.send_back(&it, "x");
    assert_eq!(runner.gh_calls().len(), 1, "the label edit never ran");
    assert_eq!(a.last_error(&it.id).as_deref(), Some("rate limited"));
}

#[test]
fn use_recommended_sends_the_phrase() {
    let runner = silent();
    actions(&runner).use_recommended(&item(MorningKind::NeedsYou, 7));
    assert_eq!(
        runner.gh_calls(),
        [
            vec![
                "issue",
                "comment",
                "7",
                "-R",
                REPO,
                "--body",
                "notchling: use recommended"
            ],
            vec![
                "issue",
                "edit",
                "7",
                "-R",
                REPO,
                "--add-label",
                "sd:queued",
                "--remove-label",
                "sd:needs-you"
            ],
        ]
    );
}

#[test]
fn answer_sends_the_text_after_the_prefix_as_one_argument() {
    let runner = silent();
    actions(&runner).answer(&item(MorningKind::NeedsYou, 7), "--repo evil/x; rm -rf /");
    assert_eq!(
        runner.gh_calls()[0][5..],
        ["--body", "notchling: --repo evil/x; rm -rf /"]
    );
    assert_eq!(runner.gh_calls()[0].len(), 7);
}

#[test]
fn retry_moves_stuck_to_queued() {
    let runner = silent();
    actions(&runner).retry(&item(MorningKind::Stuck, 7));
    assert_eq!(
        runner.gh_calls(),
        [vec![
            "issue",
            "edit",
            "7",
            "-R",
            REPO,
            "--add-label",
            "sd:queued",
            "--remove-label",
            "sd:stuck"
        ]]
    );
}

#[test]
fn close_and_drop_close_as_not_planned() {
    let runner = silent();
    let a = actions(&runner);
    a.close(&item(MorningKind::NeedsYou, 7));
    a.drop_item(&item(MorningKind::Stuck, 9));
    assert_eq!(
        runner.gh_calls(),
        [
            vec!["issue", "close", "7", "-R", REPO, "--reason", "not planned"],
            vec!["issue", "close", "9", "-R", REPO, "--reason", "not planned"],
        ]
    );
}

#[test]
fn unknown_machine_is_an_error_not_a_crash() {
    let a = MorningActions::new(silent(), Vec::new, std::env::temp_dir());
    let it = item(MorningKind::Ready, 7);
    a.merge(&it);
    assert!(a.last_error(&it.id).is_some());
    assert!(a.running().is_empty());
}

/// An issue number from a machine's status that is not a positive integer would read as an option
/// to `gh` (`-5`); no action writes with it.
#[test]
fn an_issue_number_that_is_not_positive_never_reaches_gh() {
    for n in [0, -1, -5] {
        let runner = silent();
        let a = actions(&runner);
        let (needs, stuck, ready) = (
            item(MorningKind::NeedsYou, n),
            item(MorningKind::Stuck, n),
            item(MorningKind::Ready, n),
        );
        a.close(&needs);
        a.answer(&needs, "x");
        a.use_recommended(&needs);
        a.retry(&stuck);
        a.drop_item(&stuck);
        a.send_back(&ready, "x");
        for it in [&needs, &stuck, &ready] {
            assert!(
                a.last_error(&it.id)
                    .is_some_and(|e| e.contains("is not an issue number")),
                "{n}"
            );
        }
        assert!(
            runner.calls().is_empty(),
            "{n} reached a command: {:?}",
            runner.calls()
        );
        let diff = a.diff(&ready);
        assert!(diff.contains("is not an issue number"), "{diff}");
        assert!(a.screenshots(&ready).is_empty());
        assert!(runner.calls().is_empty());
    }
}

/// A card for a repo this machine is not registered for gets no issue write at all.
#[test]
fn every_action_checks_the_repo_against_the_machines_registered_repos() {
    let foreign = |kind| item_of(kind, "evil/other", 7);
    let runner = silent();
    let calls = Arc::new(Counter::default());
    let counting = |c: &Arc<Counter>| -> MergeFunction {
        let c = c.clone();
        Arc::new(move |_, _, _, _| {
            c.bump();
            MergeOutcome::default()
        })
    };
    let a = MorningActions::new(runner.clone(), || vec![m1()], std::env::temp_dir())
        .with_plan(plan())
        .with_merge(counting(&calls))
        .with_retry_push(counting(&calls))
        .with_undo_merge(counting(&calls));
    let (ready, needs, stuck) = (
        foreign(MorningKind::Ready),
        foreign(MorningKind::NeedsYou),
        foreign(MorningKind::Stuck),
    );
    let want = "repo evil/other is not registered for m1";
    type Run<'a> = Box<dyn Fn() -> bool + 'a>;
    let actions: Vec<(&str, &MorningItem, Run)> = vec![
        ("send_back", &ready, Box::new(|| a.send_back(&ready, "x"))),
        ("answer", &needs, Box::new(|| a.answer(&needs, "x"))),
        (
            "use_recommended",
            &needs,
            Box::new(|| a.use_recommended(&needs)),
        ),
        ("retry", &stuck, Box::new(|| a.retry(&stuck))),
        ("close", &needs, Box::new(|| a.close(&needs))),
        ("drop", &stuck, Box::new(|| a.drop_item(&stuck))),
        ("merge", &ready, Box::new(|| a.merge(&ready))),
        ("undo_merge", &ready, Box::new(|| a.undo_merge(&ready))),
    ];
    for (name, it, run) in &actions {
        run();
        assert_eq!(a.last_error(&it.id).as_deref(), Some(want), "{name}");
        assert!(!a.is_running(&it.id), "{name}");
    }
    assert!(
        runner.calls().is_empty(),
        "something ran for a repo that is not registered: {:?}",
        runner.calls()
    );
    assert_eq!(calls.value(), 0);

    assert_eq!(a.diff(&ready), want);
    assert!(a.screenshots(&ready).is_empty());
    assert!(runner.calls().is_empty());
}

/// A repo name that tries to carry another `gh` flag is just an unregistered repo.
#[test]
fn a_repo_that_carries_a_flag_is_not_registered() {
    let runner = silent();
    let a = actions(&runner);
    let it = item_of(MorningKind::NeedsYou, "acme/app --repo evil/x", 7);
    a.close(&it);
    assert_eq!(
        a.last_error(&it.id).as_deref(),
        Some("repo acme/app --repo evil/x is not registered for m1")
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn an_action_for_a_machine_that_is_not_listed_says_so() {
    let runner = silent();
    let a = MorningActions::new(runner.clone(), Vec::new, std::env::temp_dir());
    let it = item(MorningKind::NeedsYou, 7);
    a.close(&it);
    assert_eq!(
        a.last_error(&it.id).as_deref(),
        Some("machine m1 is not in the machines list")
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn the_registered_repo_still_works() {
    let runner = silent();
    actions(&runner).close(&item(MorningKind::NeedsYou, 7));
    assert_eq!(
        runner.gh_calls(),
        [vec![
            "issue",
            "close",
            "7",
            "-R",
            REPO,
            "--reason",
            "not planned"
        ]]
    );
}

#[test]
fn a_missing_gh_is_an_error_and_frees_the_item() {
    let a = actions(&Arc::new(AnswerRunner::new(|_| {
        bad_code("gh: not found", 127, "")
    })));
    let it = item(MorningKind::Stuck, 7);
    a.retry(&it);
    assert_eq!(a.last_error(&it.id).as_deref(), Some("gh: not found"));
    assert!(!a.is_running(&it.id));
}
