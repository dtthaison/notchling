//! Ported from `MachinesMergeTests.swift` (second half): the guards added in fix round 1, the
//! outcome flags, retry and undo, against a scripted runner that starts nothing.

use super::fakes_jobs::{bad, machine, ok, repo, RuleRunner};
use super::merge::MergeOutcome;
use super::runner::CommandResult;
use super::tests_merge::{
    any_line, entry, happy, main_listing, merge, plan, t, MERGED, OTHER, PRE, SHA, TARGET,
};

const AHEAD: &str = "refs/remotes/origin/main..refs/heads/main";

// Guards added in fix round 1

#[test]
fn branch_switched_during_failed_check_blocks_reset() {
    let runner = happy(
        vec![
            ("-lc", bad("tests failed")),
            ("symbolic-ref HEAD", ok("refs/heads/feature\n")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main changed during the check; nothing pushed")
    );
    assert!(!any_line(&runner, |l| l.contains(" reset ")
        || l.contains(" push ")
        || l.starts_with("gh ")));
}

#[test]
fn moved_head_during_failed_check_blocks_reset() {
    let runner = happy(
        vec![("-lc", bad("tests failed"))],
        Some(vec![(
            "rev-parse HEAD",
            vec![ok(PRE), ok(MERGED), ok(OTHER)],
        )]),
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main changed during the check; nothing pushed")
    );
    assert!(!any_line(&runner, |l| l.contains(" reset ")));
}

#[test]
fn branch_switched_during_check_blocks_push() {
    let runner = happy(
        vec![("symbolic-ref HEAD", ok("refs/heads/feature\n"))],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main changed during the check; nothing pushed")
    );
    assert!(
        !o.checked,
        "the check ran on another tree than the one to push"
    );
    assert!(o.merged);
    assert!(!o.pushed);
    assert!(!o.can_retry_push());
    assert!(!any_line(&runner, |l| l.contains(" push ")
        || l.starts_with("gh ")
        || l.contains(" reset ")));
}

#[test]
fn commit_added_during_check_blocks_push() {
    let runner = happy(
        vec![],
        Some(vec![(
            "rev-parse HEAD",
            vec![
                ok(&format!("{PRE}\n")),
                ok(&format!("{MERGED}\n")),
                ok(&format!("{OTHER}\n")),
            ],
        )]),
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main changed during the check; nothing pushed")
    );
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.starts_with("gh ")));
}

/// I3: the check built a tree with an edit in it; what would be pushed is a different tree.
#[test]
fn local_changes_made_during_the_check_block_the_push_and_keep_the_merge_local() {
    let runner = happy(
        vec![],
        Some(vec![
            ("status --porcelain", vec![ok(""), ok(" M f.txt\n")]),
            ("rev-parse HEAD", vec![ok(PRE), ok(MERGED)]),
        ]),
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main has local changes made during the check; nothing pushed")
    );
    assert!(o.merged);
    assert!(!o.checked);
    assert!(!o.pushed);
    assert!(
        o.can_undo(),
        "the merge stays local, so Undo is the way out"
    );
    assert!(!o.can_retry_push());
    let lines = runner.lines();
    assert_eq!(
        lines[lines.len() - 3..],
        [
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD"),
            t("status --porcelain")
        ]
    );
    assert!(!any_line(&runner, |l| l.contains(" push ")
        || l.starts_with("gh ")
        || l.contains(" reset ")));
}

/// An edit made during a failed check does not stop the reset: `--keep` leaves the edit alone.
#[test]
fn a_failed_check_still_puts_main_back_when_the_tree_got_dirty_meanwhile() {
    let runner = happy(
        vec![("-lc", bad("tests failed"))],
        Some(vec![
            ("status --porcelain", vec![ok(""), ok(" M f.txt\n")]),
            ("rev-parse HEAD", vec![ok(PRE), ok(MERGED)]),
        ]),
    );
    let o = merge(&entry(), &runner);
    assert!(!o.merged, "reset --keep ran, and kept the edit");
    assert_eq!(
        runner.lines().last(),
        Some(&t(&format!("reset --keep {PRE}")))
    );
}

#[test]
fn failed_abort_stops_before_gh() {
    let runner = happy(
        vec![
            ("merge --no-ff", bad("CONFLICT")),
            ("diff --name-only", ok("a.swift\n")),
            ("merge --abort", bad("fatal: cannot abort")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some(format!("merge could not be aborted in {TARGET}; fix it by hand").as_str())
    );
    assert!(!any_line(&runner, |l| l.starts_with("gh ")));
}

#[test]
fn failed_diff_is_reported_and_labels_nothing() {
    let runner = happy(
        vec![
            ("merge --no-ff", bad("fatal: weird")),
            ("diff --name-only", bad("fatal: diff broke")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("could not be listed")));
    assert!(o.conflict_files.is_empty());
    assert!(!any_line(&runner, |l| l.starts_with("gh ")));
    assert_eq!(runner.lines().last(), Some(&t("merge --abort")));
}

/// The note on the issue comes from the machine's branch name and the file names git listed, and
/// goes to `gh` as one argument after `--body`.
#[test]
fn a_conflict_note_is_one_argument() {
    let runner = happy(
        vec![
            ("merge --no-ff", bad("CONFLICT")),
            ("diff --name-only", ok("a; touch x\n--help\n")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(o.conflict_files, vec!["a; touch x", "--help"]);
    let calls = runner.calls();
    let comment = calls
        .iter()
        .find(|c| c.contains(&"comment".to_string()))
        .expect("a comment");
    assert_eq!(comment.len(), 8);
    assert_eq!(comment[6], "--body");
    assert!(comment[7].contains("a; touch x, --help"));
}

// Outcome flags (C17, C18)

#[test]
fn outcome_flags_decide_which_buttons_show() {
    // (merged, checked, pushed) -> (can_retry_push, can_undo)
    let table = [
        (false, false, false, false, false), // nothing merged
        (true, false, false, false, true),   // merged locally, never checked: undo only
        (true, true, false, true, true),     // checked, push failed: both
        (true, true, true, false, false),    // pushed
        (false, true, false, false, false),
    ];
    for (merged, checked, pushed, retry, undo) in table {
        let o = MergeOutcome {
            merged,
            checked,
            pushed,
            ..MergeOutcome::default()
        };
        assert_eq!(
            o.can_retry_push(),
            retry,
            "merged {merged} checked {checked} pushed {pushed}"
        );
        assert_eq!(
            o.can_undo(),
            undo,
            "merged {merged} checked {checked} pushed {pushed}"
        );
    }
}

// Retry

/// A target where the failed push left exactly one merge of this branch on top of origin/main.
fn retry_runner(extra: Vec<(&'static str, CommandResult)>) -> RuleRunner {
    let mut rules = extra;
    rules.extend([
        ("worktree list", ok(main_listing())),
        ("status --porcelain", ok("")),
        ("symbolic-ref HEAD", ok("refs/heads/main\n")),
        ("rev-parse HEAD^2", ok(&format!("{SHA}\n"))),
        ("--first-parent --count", ok("1\n")),
        ("rev-parse HEAD", ok(&format!("{MERGED}\n"))),
    ]);
    RuleRunner::new(rules, vec![])
}

fn retry(runner: &RuleRunner) -> MergeOutcome {
    plan().retry_push(&entry(), &machine("m1"), &repo(), runner)
}

#[test]
fn retry_push_repeats_only_push_and_close() {
    let runner = retry_runner(vec![]);
    let o = retry(&runner);
    assert!(o.pushed);
    assert!(o.closed);
    assert_eq!(o.commit.as_deref(), Some(MERGED));
    assert_eq!(
        runner.lines(),
        vec![
            "git -C /Users/me/code/app worktree list --porcelain".to_string(),
            t("status --porcelain"),
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD^2"),
            t(&format!("rev-list --first-parent --count {AHEAD}")),
            t("rev-parse HEAD"),
            t("push origin refs/heads/main:refs/heads/main"),
            format!("gh issue close 7 -R acme/app --comment notchling: merged in {MERGED}"),
            "gh issue edit 7 -R acme/app --remove-label sd:ready".to_string(),
        ]
    );
}

fn assert_retry_refuses(extra: Vec<(&'static str, CommandResult)>) {
    let runner = retry_runner(extra);
    let o = retry(&runner);
    assert!(!o.pushed);
    assert!(o.error.is_some());
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.starts_with("gh ")));
}

#[test]
fn retry_push_refuses_dirty_tree() {
    assert_retry_refuses(vec![("status --porcelain", ok(" M a.swift\n"))]);
}

#[test]
fn retry_push_refuses_when_not_on_main() {
    assert_retry_refuses(vec![("symbolic-ref HEAD", ok("refs/heads/feature\n"))]);
}

#[test]
fn retry_push_refuses_when_head_is_not_this_merge() {
    assert_retry_refuses(vec![(
        "rev-parse HEAD^2",
        ok("9999999999999999999999999999999999999999\n"),
    )]);
}

#[test]
fn retry_push_refuses_when_head_is_no_merge() {
    assert_retry_refuses(vec![("rev-parse HEAD^2", bad("fatal: ambiguous argument"))]);
}

#[test]
fn retry_push_refuses_other_unpushed_commits() {
    assert_retry_refuses(vec![("--first-parent --count", ok("2\n"))]);
}

#[test]
fn retry_push_refuses_when_nothing_is_ahead() {
    assert_retry_refuses(vec![("--first-parent --count", ok("0\n"))]);
}

#[test]
fn retry_push_refuses_an_issue_number_that_is_not_positive() {
    let runner = retry_runner(vec![]);
    let mut e = entry();
    e.issue = -3;
    let o = plan().retry_push(&e, &machine("m1"), &repo(), &runner);
    assert!(o.error.is_some() && !o.pushed);
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.starts_with("gh ")));
}

// Undo (C18)

/// A target whose main holds exactly this item's merge on top of origin.
fn undo_runner(extra: Vec<(&'static str, CommandResult)>) -> RuleRunner {
    let mut rules = extra;
    rules.extend([
        ("worktree list", ok(main_listing())),
        ("status --porcelain", ok("")),
        ("symbolic-ref HEAD", ok("refs/heads/main\n")),
        (
            "rev-list --parents -n 1 HEAD",
            ok(&format!("{MERGED} {PRE} {SHA}\n")),
        ),
        ("--first-parent --count", ok("1\n")),
    ]);
    RuleRunner::new(rules, vec![])
}

fn undo(runner: &RuleRunner) -> MergeOutcome {
    plan().undo_merge(&entry(), &machine("m1"), &repo(), runner)
}

#[test]
fn undo_goes_back_to_the_first_parent_after_every_guard() {
    let runner = undo_runner(vec![]);
    let o = undo(&runner);
    assert_eq!(o.error, None);
    assert!(!o.merged);
    assert!(!o.can_undo());
    assert_eq!(
        runner.lines(),
        vec![
            "git -C /Users/me/code/app worktree list --porcelain".to_string(),
            t("status --porcelain"),
            t("symbolic-ref HEAD"),
            t("rev-list --parents -n 1 HEAD"),
            t("fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main"),
            t(&format!("rev-list --first-parent --count {AHEAD}")),
            t("reset --keep HEAD^1"),
        ]
    );
    assert_eq!(runner.recs()[4].timeout.as_secs(), 300);
    assert!(!any_line(&runner, |l| l.contains(" push ")
        || l.starts_with("gh ")
        || l.contains("--hard")));
}

fn assert_undo_refuses(extra: Vec<(&'static str, CommandResult)>, mentions: Option<&str>) {
    let runner = undo_runner(extra);
    let o = undo(&runner);
    assert!(o.error.is_some());
    if let Some(m) = mentions {
        assert!(
            o.error.as_deref().is_some_and(|e| e.contains(m)),
            "{:?}",
            o.error
        );
    }
    assert!(!any_line(&runner, |l| l.contains(" reset ")
        || l.contains(" push ")
        || l.starts_with("gh ")));
}

#[test]
fn undo_refuses_dirty_tree() {
    assert_undo_refuses(
        vec![("status --porcelain", ok(" M a.swift\n"))],
        Some("local changes"),
    );
}

#[test]
fn undo_refuses_when_not_on_main() {
    assert_undo_refuses(
        vec![("symbolic-ref HEAD", ok("refs/heads/feature\n"))],
        Some("not on main"),
    );
}

#[test]
fn undo_refuses_when_head_is_another_branchs_merge() {
    assert_undo_refuses(
        vec![(
            "rev-list --parents -n 1 HEAD",
            ok(&format!(
                "{MERGED} {PRE} 9999999999999999999999999999999999999999\n"
            )),
        )],
        None,
    );
}

#[test]
fn undo_refuses_when_head_is_no_merge() {
    assert_undo_refuses(
        vec![(
            "rev-list --parents -n 1 HEAD",
            ok(&format!("{MERGED} {PRE}\n")),
        )],
        None,
    );
}

#[test]
fn undo_refuses_an_octopus_merge() {
    assert_undo_refuses(
        vec![(
            "rev-list --parents -n 1 HEAD",
            ok(&format!(
                "{MERGED} {PRE} {SHA} 5555555555555555555555555555555555555555\n"
            )),
        )],
        None,
    );
}

#[test]
fn undo_refuses_when_the_merge_is_not_alone_on_top_of_origin() {
    assert_undo_refuses(
        vec![("--first-parent --count", ok("2\n"))],
        Some("not on origin"),
    );
}

#[test]
fn undo_refuses_when_origin_cannot_be_fetched() {
    assert_undo_refuses(
        vec![("fetch --no-tags origin", bad("could not resolve host"))],
        Some("could not resolve host"),
    );
}

#[test]
fn undo_reports_a_failed_reset() {
    let runner = undo_runner(vec![(
        "reset --keep",
        bad("error: Entry 'f.txt' not uptodate. Cannot merge."),
    )]);
    let o = undo(&runner);
    assert!(
        o.error
            .as_deref()
            .is_some_and(|e| e.contains("Cannot merge")),
        "{:?}",
        o.error
    );
    assert_eq!(runner.lines().last(), Some(&t("reset --keep HEAD^1")));
}
