use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::actions::{MergeFunction, MorningActions};
use super::fakes_jobs::{AnswerRunner, Counter, Gate};
use super::merge::{MergeOutcome, MergePlan};
use super::morning::{item_id, MorningEntry, MorningItem, MorningKind};
use super::registry::{Machine, MachineRepo, SdWorkerSettings};
use super::runner::CommandResult;
use super::status::{NeedsYouEntry, ReadyEntry, StuckEntry};

pub(super) const REPO: &str = "acme/app";

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn m1() -> Machine {
    let mut m = Machine::new("m1", "m1.tail", "agent");
    let mut job = SdWorkerSettings::new("m1");
    job.sd_path = "/sd/sd.py".into();
    job.repos = vec![MachineRepo {
        github: REPO.into(),
        local_path: "/work/app".into(),
        check_command: "just check".into(),
        machine_path: None,
        verify_skill: None,
    }];
    m.set_sd_worker(Some(job));
    m
}

pub(super) fn plan() -> MergePlan {
    MergePlan {
        home: "/Users/me".into(),
        merge_root: "/data/notchling/daemon/machines/merge".into(),
        environment: Default::default(),
        git: "/usr/bin/git".into(),
        gh: "/usr/bin/gh".into(),
    }
}

pub(super) fn ready(n: i64) -> ReadyEntry {
    ReadyEntry {
        repo: REPO.into(),
        issue: n,
        branch: format!("sd/{n}"),
        commit: "abc".into(),
        worktree: format!("/wt/{n}"),
        evidence_dir: format!("/wt/{n}/evidence"),
        ready_at: None,
    }
}

pub(super) fn item_of(kind: MorningKind, repo: &str, n: i64) -> MorningItem {
    let entry = match kind {
        MorningKind::Ready => MorningEntry::Ready(ReadyEntry {
            repo: repo.into(),
            ..ready(n)
        }),
        MorningKind::NeedsYou => MorningEntry::NeedsYou(NeedsYouEntry {
            repo: repo.into(),
            issue: n,
            reason: "question".into(),
            ..NeedsYouEntry::default()
        }),
        MorningKind::Stuck => MorningEntry::Stuck(StuckEntry {
            repo: repo.into(),
            issue: n,
            reason: "gave up".into(),
            ..StuckEntry::default()
        }),
    };
    MorningItem {
        id: item_id("m1", repo, n),
        kind,
        machine: "m1".into(),
        repo: repo.into(),
        issue: n,
        title: "t".into(),
        entry,
        comment: None,
    }
}

pub(super) fn item(kind: MorningKind, n: i64) -> MorningItem {
    item_of(kind, REPO, n)
}

pub(super) fn ready_item(entry: ReadyEntry) -> MorningItem {
    MorningItem {
        entry: MorningEntry::Ready(entry.clone()),
        ..item(MorningKind::Ready, entry.issue)
    }
}

pub(super) fn merge_fn(o: MergeOutcome) -> MergeFunction {
    Arc::new(move |_, _, _, _| o.clone())
}

pub(super) fn passed() -> MergeOutcome {
    MergeOutcome {
        pushed: true,
        closed: true,
        ..MergeOutcome::default()
    }
}

pub(super) fn temp_cache(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "morning-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

pub(super) fn actions_over(
    runner: &Arc<AnswerRunner>,
    machines: Vec<Machine>,
    cache: PathBuf,
) -> MorningActions {
    MorningActions::new(runner.clone(), move || machines.clone(), cache)
        .with_plan(plan())
        .with_merge(merge_fn(passed()))
        .with_retry_push(merge_fn(passed()))
        .with_undo_merge(merge_fn(MergeOutcome::default()))
}

pub(super) fn actions(runner: &Arc<AnswerRunner>) -> MorningActions {
    actions_over(runner, vec![m1()], temp_cache("a"))
}

pub(super) fn silent() -> Arc<AnswerRunner> {
    Arc::new(AnswerRunner::silent())
}

#[test]
fn stale_outcome_of_another_commit_shows_nothing() {
    let a = actions(&silent()).with_merge(merge_fn(MergeOutcome {
        fetched: true,
        merged: true,
        checked: true,
        error: Some("push rejected".into()),
        ..MergeOutcome::default()
    }));
    let first = item(MorningKind::Ready, 7);
    assert!(a.merge(&first));
    assert_eq!(
        a.outcome(&first).map(|o| o.merged),
        Some(true),
        "the outcome of this commit shows (checklist and Retry push)"
    );

    let mut entry = ready(7);
    entry.commit = "def".into();
    let returned = ready_item(entry);
    assert_eq!(
        a.outcome(&returned),
        None,
        "no checklist and no Retry push for a commit that was never merged"
    );

    a.prune_outcomes(std::slice::from_ref(&returned));
    assert!(
        !a.outcomes().contains_key(&first.id),
        "the stale outcome is dropped"
    );
    assert!(a.merge(&first));
    assert!(a.outcome(&first).is_some());
    a.prune_outcomes(&[]);
    assert!(
        a.outcomes().is_empty(),
        "an item that left the list loses its outcome"
    );
}

#[test]
fn one_action_per_item() {
    let counter = Arc::new(Counter::default());
    let gate = Arc::new(Gate::default());
    let (c, g) = (counter.clone(), gate.clone());
    let a = actions(&silent()).with_merge(Arc::new(move |_, _, _, _| {
        c.bump();
        g.wait();
        passed()
    }));
    let it = item(MorningKind::Ready, 7);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| a.merge(&it));
        counter.wait_for_first();
        assert!(a.is_running(&it.id));
        assert!(
            !a.merge(&it),
            "the second merge on the same item was ignored"
        );
        gate.open();
        assert!(first.join().expect("the merge ends"));
    });
    assert_eq!(counter.value(), 1);
    assert!(a.running().is_empty());
    assert_eq!(a.outcomes().get(&it.id), Some(&passed()));
    assert_eq!(a.last_error(&it.id), None);

    assert!(a.merge(&item(MorningKind::Ready, 8)));
    assert!(a.merge(&it));
    assert_eq!(counter.value(), 3);
}

#[test]
fn failed_action_frees_the_item_and_keeps_the_error() {
    let a = actions(&silent()).with_merge(merge_fn(MergeOutcome::failed(
        "main has local changes in /work/app".into(),
    )));
    let it = item(MorningKind::Ready, 7);
    a.merge(&it);
    assert!(!a.is_running(&it.id));
    assert_eq!(
        a.last_error(&it.id).as_deref(),
        Some("main has local changes in /work/app")
    );

    let failing = Arc::new(AnswerRunner::new(|_| {
        CommandResult::new(1, "", "HTTP 404\n")
    }));
    let b = actions(&failing);
    let stuck = item(MorningKind::Stuck, 7);
    b.retry(&stuck);
    assert!(b.running().is_empty());
    assert_eq!(b.last_error(&stuck.id).as_deref(), Some("HTTP 404"));
}

#[test]
fn a_gh_failure_with_no_text_or_a_timeout_is_named() {
    for (code, err, want) in [
        (1, "", "gh issue close exited 1"),
        (124, "", "gh issue close timed out"),
    ] {
        let a = actions(&Arc::new(AnswerRunner::new(move |_| {
            CommandResult::new(code, "", err)
        })));
        let it = item(MorningKind::NeedsYou, 7);
        a.close(&it);
        assert_eq!(a.last_error(&it.id).as_deref(), Some(want));
    }
}

#[test]
fn merge_with_a_push_that_failed_offers_a_retry() {
    let calls = Arc::new(Counter::default());
    let c = calls.clone();
    let a = actions(&silent())
        .with_merge(merge_fn(MergeOutcome {
            merged: true,
            checked: true,
            ..MergeOutcome::default()
        }))
        .with_retry_push(Arc::new(move |_, _, _, _| {
            c.bump();
            passed()
        }));
    let it = item(MorningKind::Ready, 7);
    a.merge(&it);
    assert_eq!(a.last_error(&it.id).as_deref(), Some("main was not pushed"));
    a.retry_push(&it);
    assert_eq!(calls.value(), 1);
    assert_eq!(
        a.last_error(&it.id),
        None,
        "a new action clears the old error"
    );
}

#[test]
fn retry_push_refuses_a_merge_that_was_never_checked() {
    let calls = Arc::new(Counter::default());
    let outcomes = [
        MergeOutcome {
            merged: true,
            error: Some(
                "check failed and main could not be put back; fix it by hand in /work/app".into(),
            ),
            ..MergeOutcome::default()
        },
        MergeOutcome {
            merged: true,
            error: Some("main has local changes made during the check; nothing pushed".into()),
            ..MergeOutcome::default()
        },
        MergeOutcome::failed("main has local changes in /work/app".into()),
    ];
    for outcome in outcomes {
        let c = calls.clone();
        let a = actions(&silent())
            .with_merge(merge_fn(outcome.clone()))
            .with_retry_push(Arc::new(move |_, _, _, _| {
                c.bump();
                passed()
            }));
        let it = item(MorningKind::Ready, 7);
        a.merge(&it);
        a.retry_push(&it);
        assert_eq!(calls.value(), 0, "{outcome:?}");
        assert!(a.last_error(&it.id).is_some());
        assert!(!a.is_running(&it.id));
    }
    let c = calls.clone();
    let b = actions(&silent()).with_retry_push(Arc::new(move |_, _, _, _| {
        c.bump();
        passed()
    }));
    b.retry_push(&item(MorningKind::Ready, 7));
    assert_eq!(calls.value(), 0);
}

#[test]
fn undo_merge_forgets_the_outcome_and_leaves_a_note() {
    let seen = Arc::new(Counter::default());
    let s = seen.clone();
    let a = actions(&silent())
        .with_merge(merge_fn(MergeOutcome {
            fetched: true,
            merged: true,
            checked: true,
            commit: Some("m".into()),
            error: Some("push failed: rejected".into()),
            ..MergeOutcome::default()
        }))
        .with_undo_merge(Arc::new(
            move |e: &ReadyEntry, m: &Machine, r: &MachineRepo, _| {
                s.bump();
                assert_eq!(e.issue, 7);
                assert_eq!(m.name, "m1");
                assert_eq!(r.github, "acme/app");
                MergeOutcome::default()
            },
        ));
    let it = item(MorningKind::Ready, 7);
    a.merge(&it);
    assert_eq!(a.outcome(&it).map(|o| o.can_undo()), Some(true));
    a.undo_merge(&it);
    assert_eq!(seen.value(), 1);
    assert_eq!(
        a.outcome(&it),
        None,
        "no checklist, no Retry push, no Undo for a merge that is gone"
    );
    assert_eq!(a.last_error(&it.id), None);
    assert!(a.note(&it.id).is_some());
    assert!(!a.is_running(&it.id));

    a.merge(&it);
    assert_eq!(a.note(&it.id), None);
    a.undo_merge(&it);
    a.prune_outcomes(&[]);
    assert!(a.notes().is_empty());
}

#[test]
fn undo_merge_that_git_refuses_keeps_the_outcome_and_shows_the_reason() {
    let why = "cannot undo the merge: main has local changes in /work/app; put them away first";
    let a = actions(&silent())
        .with_merge(merge_fn(MergeOutcome {
            fetched: true,
            merged: true,
            checked: true,
            error: Some("push failed: rejected".into()),
            ..MergeOutcome::default()
        }))
        .with_undo_merge(merge_fn(MergeOutcome::failed(why.into())));
    let it = item(MorningKind::Ready, 7);
    a.merge(&it);
    a.undo_merge(&it);
    assert_eq!(a.last_error(&it.id).as_deref(), Some(why));
    assert_eq!(
        a.outcome(&it).map(|o| o.can_undo()),
        Some(true),
        "the merge is still there"
    );
    assert_eq!(a.note(&it.id), None);
}

#[test]
fn undo_merge_is_one_action_per_item_and_only_for_a_ready_branch() {
    let calls = Arc::new(Counter::default());
    let gate = Arc::new(Gate::default());
    let (c, g) = (calls.clone(), gate.clone());
    let a = actions(&silent()).with_undo_merge(Arc::new(move |_, _, _, _| {
        c.bump();
        g.wait();
        MergeOutcome::default()
    }));
    let it = item(MorningKind::Ready, 7);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| a.undo_merge(&it));
        calls.wait_for_first();
        assert!(a.is_running(&it.id));
        assert!(!a.undo_merge(&it));
        gate.open();
        assert!(first.join().expect("the undo ends"));
    });
    assert_eq!(calls.value(), 1);

    let stuck = item(MorningKind::Stuck, 7);
    a.undo_merge(&stuck);
    assert!(a.last_error(&stuck.id).is_some());
    assert_eq!(calls.value(), 1);

    let unknown = MorningActions::new(silent(), Vec::new, std::env::temp_dir());
    unknown.undo_merge(&it);
    assert!(unknown.last_error(&it.id).is_some());
}
