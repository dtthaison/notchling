//! Ported from `MachinesMergeTests.swift` (first half): the target, the refusals, the fetch, the
//! refs, the check's shell and the happy path, against a scripted runner that starts nothing.
//! The guards, retry and undo are in `tests_merge_b.rs`.

use std::collections::BTreeMap;

use super::fakes_jobs::{bad, machine, ok, repo, RuleRunner};
use super::merge::{MergeOutcome, MergePlan, MergeTarget};
use super::runner::CommandResult;
use super::status::ReadyEntry;

pub(super) const TARGET: &str = "/Users/me/code/app";
pub(super) const SHA: &str = "1111111111111111111111111111111111111111";
pub(super) const MERGED: &str = "2222222222222222222222222222222222222222";
pub(super) const PRE: &str = "0000000000000000000000000000000000000abc";
pub(super) const OTHER: &str = "3333333333333333333333333333333333333333";

pub(super) fn plan() -> MergePlan {
    MergePlan {
        home: "/Users/me".into(),
        merge_root: "/data/notchling/daemon/machines/merge".into(),
        // no SHELL: the check runs in /bin/zsh
        environment: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())]),
        git: "/usr/bin/git".into(),
        gh: "/usr/bin/gh".into(),
    }
}

pub(super) fn entry() -> ReadyEntry {
    ReadyEntry {
        repo: "acme/app".into(),
        issue: 7,
        branch: "sd/issue-7".into(),
        commit: SHA.into(),
        worktree: "/work/app-7".into(),
        evidence_dir: String::new(),
        ready_at: None,
    }
}

pub(super) fn main_listing() -> &'static str {
    "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/main\n"
}

/// `git -C <target> <rest>`
pub(super) fn t(rest: &str) -> String {
    format!("git -C {TARGET} {rest}")
}

type Rules = Vec<(&'static str, CommandResult)>;
type Queues = Vec<(&'static str, Vec<CommandResult>)>;

/// Rules for a run where every step passes.
fn happy_rules() -> Rules {
    vec![
        ("worktree list", ok(main_listing())),
        ("check-ref-format --branch sd/issue-7", ok("sd/issue-7\n")),
        ("rev-parse refs/machines/m1/7", ok(&format!("{SHA}\n"))),
        (
            "--count refs/remotes/origin/main..refs/heads/main",
            ok("0\n"),
        ),
        ("symbolic-ref HEAD", ok("refs/heads/main\n")),
        ("merge --no-ff", ok("Merge made by the 'ort' strategy.\n")),
    ]
}

/// `rev-parse HEAD` answers the sha before the merge, then the merge commit (also at each re-check).
pub(super) fn happy_queues() -> Queues {
    vec![(
        "rev-parse HEAD",
        vec![ok(&format!("{PRE}\n")), ok(&format!("{MERGED}\n"))],
    )]
}

pub(super) fn happy(extra: Rules, queues: Option<Queues>) -> RuleRunner {
    let mut rules = extra;
    rules.extend(happy_rules());
    RuleRunner::new(rules, queues.unwrap_or_else(happy_queues))
}

pub(super) fn merge(e: &ReadyEntry, runner: &RuleRunner) -> MergeOutcome {
    plan().merge(e, &machine("m1"), &repo(), runner)
}

pub(super) fn any_line(runner: &RuleRunner, f: impl Fn(&str) -> bool) -> bool {
    runner.lines().iter().any(|l| f(l))
}

// Target

#[test]
fn merge_target_prefers_clean_main_worktree() {
    let list = "worktree /Users/me/code/app-feature\nHEAD aaa\nbranch refs/heads/feature\n\n\
                worktree /Users/me/code/app main\nHEAD bbb\nbranch refs/heads/main\n\n\
                worktree /tmp/other\nHEAD ccc\nbranch refs/heads/main-old\n\n";
    assert_eq!(
        plan().target(&repo(), list),
        MergeTarget::Existing("/Users/me/code/app main".into())
    );
}

#[test]
fn merge_target_makes_own_when_main_unchecked() {
    let list = "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/feature\n\nworktree /x\nHEAD bbb\ndetached\n";
    assert_eq!(
        plan().target(&repo(), list),
        MergeTarget::Own("/data/notchling/daemon/machines/merge/acme-app".into()),
        "under the data root, never the home's Library"
    );
}

#[test]
fn a_bare_or_prunable_main_is_not_a_target() {
    for flag in [
        "bare",
        "prunable",
        "prunable gitdir file points to non-existent location",
    ] {
        let list = format!("worktree /x\nHEAD aaa\nbranch refs/heads/main\n{flag}\n");
        assert!(
            matches!(plan().target(&repo(), &list), MergeTarget::Own(_)),
            "{flag}"
        );
    }
}

#[test]
fn own_worktree_is_made_under_the_data_folder() {
    let list = "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/feature\n";
    let runner = RuleRunner::new(
        vec![
            ("worktree list", ok(list)),
            ("check-ref-format", ok("sd/issue-7")),
            ("rev-parse refs/machines/m1/7", ok(SHA)),
            ("--count refs/remotes/origin/main..refs/heads/main", ok("0")),
        ],
        happy_queues(),
    );
    let _ = merge(&entry(), &runner);
    let own = "/data/notchling/daemon/machines/merge/acme-app";
    let lines = runner.lines();
    assert_eq!(lines[1], "mkdir -p /data/notchling/daemon/machines/merge");
    assert_eq!(lines[2], format!("git -C {TARGET} worktree add {own} main"));
    assert_eq!(
        lines[3],
        format!("git -C {own} check-ref-format --branch sd/issue-7")
    );
    assert_eq!(lines[4], format!("git -C {own} status --porcelain"));
    assert!(!lines.join("").contains('~'));
}

// Refusals

#[test]
fn dirty_main_refuses() {
    let runner = RuleRunner::new(
        vec![
            ("worktree list", ok(main_listing())),
            ("check-ref-format", ok("sd/issue-7")),
            ("status --porcelain", ok(" M file.swift\n")),
        ],
        vec![],
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main has local changes in /Users/me/code/app")
    );
    assert!(!o.fetched);
    assert_eq!(
        runner.lines(),
        vec![
            "git -C /Users/me/code/app worktree list --porcelain".to_string(),
            t("check-ref-format --branch sd/issue-7"),
            t("status --porcelain"),
        ]
    );
}

#[test]
fn fetch_failure_changes_nothing() {
    let runner = happy(
        vec![("fetch --no-tags ssh://", bad("ssh: could not resolve host"))],
        None,
    );
    let o = merge(&entry(), &runner);
    assert!(!o.fetched);
    assert!(!o.merged);
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("could not resolve host")));
    assert!(!any_line(&runner, |l| l.contains(" merge --no-ff")
        || l.contains(" push ")));
    assert!(!any_line(&runner, |l| l.starts_with("gh ")));
    assert!(!any_line(&runner, |l| l.contains(" reset ")));
}

#[test]
fn moved_branch_refuses() {
    let runner = happy(
        vec![(
            "rev-parse refs/machines/m1/7",
            ok("9999999999999999999999999999999999999999\n"),
        )],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("branch moved on the machine; refresh")
    );
    assert!(o.fetched);
    assert!(!o.merged);
    assert!(!any_line(&runner, |l| l.contains(" merge --no-ff")
        || l.contains(" push ")
        || l.starts_with("gh ")));
}

// Branch names (C16)

#[test]
fn a_branch_that_looks_like_an_option_never_reaches_git() {
    for branch in ["-x", "--upload-pack=sh", "", "a b", "a\nb", "a\u{7f}b"] {
        let runner = happy(vec![], None);
        let mut e = entry();
        e.branch = branch.into();
        let o = merge(&e, &runner);
        assert!(o.error.is_some(), "'{branch}' was accepted");
        assert!(
            runner.lines().is_empty(),
            "'{branch}' reached a command: {:?}",
            runner.lines()
        );
    }
}

#[test]
fn git_must_accept_the_branch_name_and_print_it_back_unchanged() {
    // refused by git
    let runner = happy(
        vec![(
            "check-ref-format",
            bad("fatal: 'a..b' is not a valid branch name"),
        )],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("\"sd/issue-7\" is not a branch name git accepts")
    );
    assert_eq!(
        runner.lines().last(),
        Some(&t("check-ref-format --branch sd/issue-7"))
    );
    assert!(!o.fetched);

    // accepted, but expanded to another name (`@{-1}` is "the branch before this one")
    let runner = happy(vec![("check-ref-format", ok("some-other-branch\n"))], None);
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("\"sd/issue-7\" is not a branch name git accepts")
    );
    assert!(!any_line(&runner, |l| l.contains("fetch")
        || l.contains("merge")
        || l.contains("push")));
}

// Fetch over ssh (C20)

/// BatchMode and a short connect timeout: a prompt or a dead host must not hold the card for the
/// fetch's 300 s.
#[test]
fn the_fetch_from_the_machine_runs_with_batch_mode_ssh() {
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    let recs = runner.recs();
    let machine_fetches: Vec<_> = recs
        .iter()
        .filter(|r| {
            r.argv.contains(&"fetch".to_string()) && r.argv.iter().any(|a| a.starts_with("ssh://"))
        })
        .collect();
    assert_eq!(machine_fetches.len(), 1);
    assert_eq!(
        machine_fetches[0].env,
        vec![(
            "GIT_SSH_COMMAND".to_string(),
            "ssh -o BatchMode=yes -o ConnectTimeout=8".to_string()
        )]
    );
    // the other fetch is origin's; it keeps whatever ssh config the user has
    let origin = recs
        .iter()
        .find(|r| r.argv.contains(&"fetch".to_string()) && r.argv.contains(&"origin".to_string()))
        .expect("origin fetch");
    assert!(origin.env.is_empty());
}

/// The worktree path is the machine's word, and it becomes part of an ssh URL.
#[test]
fn a_worktree_path_that_is_not_absolute_never_reaches_git() {
    for path in [
        "",
        ".evil.com/x",
        "@other/x",
        "relative/dir",
        "-oProxyCommand=x",
    ] {
        let runner = happy(vec![], None);
        let mut e = entry();
        e.worktree = path.into();
        let o = merge(&e, &runner);
        assert!(
            o.error
                .as_deref()
                .is_some_and(|e| e.contains("not an absolute path")),
            "{path}: {:?}",
            o.error
        );
        assert!(
            runner.lines().is_empty(),
            "{path} reached a command: {:?}",
            runner.lines()
        );
        assert!(!o.fetched);
    }
}

/// An absolute path with a quote, a space, a `;` or a `..` is not passed to a command either.
#[test]
fn a_worktree_path_with_odd_characters_never_reaches_git() {
    for path in [
        "/w/a b",
        "/w/a;touch /tmp/x",
        "/w/$(id)",
        "/w/../etc",
        "/w/it's",
        "/w/a\nb",
        "/w/`id`",
    ] {
        let runner = happy(vec![], None);
        let mut e = entry();
        e.worktree = path.into();
        let o = merge(&e, &runner);
        assert!(o.error.is_some(), "{path:?} was accepted");
        assert!(runner.lines().is_empty(), "{path:?} reached a command");
        assert!(!o.fetched);
    }
}

/// An issue number from a machine must be a positive integer: `-5` would read as an option.
#[test]
fn an_issue_number_that_is_not_positive_never_reaches_a_command() {
    for issue in [0, -1, -5] {
        let runner = happy(vec![], None);
        let mut e = entry();
        e.issue = issue;
        let o = merge(&e, &runner);
        assert!(o.error.is_some(), "{issue}");
        assert!(runner.lines().is_empty(), "{issue} reached a command");
    }
}

/// The commit is merged only when it is what git printed for the fetched ref.
#[test]
fn a_commit_that_is_not_what_git_printed_is_never_merged() {
    for commit in ["--upload-pack=x", "", "main", "abc"] {
        let runner = happy(vec![], None);
        let mut e = entry();
        e.commit = commit.into();
        let o = merge(&e, &runner);
        assert_eq!(
            o.error.as_deref(),
            Some("branch moved on the machine; refresh"),
            "{commit:?}"
        );
        assert!(!any_line(&runner, |l| l.contains(" merge --no-ff")));
    }
}

// Main ahead of origin (C18)

#[test]
fn unpushed_main_refuses_without_suggesting_a_push() {
    let range = "--count refs/remotes/origin/main..refs/heads/main";
    let runner = happy(
        vec![("--first-parent --count", ok("2\n")), (range, ok("2\n"))],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main has 2 commit(s) that are not on origin; push or undo them yourself")
    );
    assert!(!o.fetched);
    assert!(!o.merged);
    assert!(!o.can_undo());
    assert!(!any_line(&runner, |l| l.contains(" push ")
        || l.contains(" reset ")
        || l.contains(" merge --no-ff")));
}

#[test]
fn main_ahead_by_exactly_this_items_merge_is_reported_and_offers_undo_never_a_retry() {
    let range = "--count refs/remotes/origin/main..refs/heads/main";
    let runner = happy(
        vec![
            ("--first-parent --count", ok("1\n")),
            (range, ok("2\n")),
            (
                "rev-list --parents -n 1 refs/heads/main",
                ok(&format!("{MERGED} {PRE} {SHA}\n")),
            ),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert!(
        o.error
            .as_deref()
            .is_some_and(|e| e.starts_with("merged locally, not pushed")),
        "{:?}",
        o.error
    );
    assert!(o.merged);
    assert!(!o.checked, "its check never ran");
    assert!(!o.pushed);
    assert_eq!(o.commit.as_deref(), Some(MERGED));
    assert!(o.can_undo());
    assert!(!o.can_retry_push());
    assert!(!any_line(&runner, |l| {
        l.contains(" push ")
            || l.contains(" reset ")
            || l.starts_with("gh ")
            || l.contains("fetch --no-tags ssh")
    }));
}

#[test]
fn main_ahead_by_another_branchs_merge_is_not_this_items() {
    let range = "--count refs/remotes/origin/main..refs/heads/main";
    let other = "4444444444444444444444444444444444444444";
    let runner = happy(
        vec![
            ("--first-parent --count", ok("1\n")),
            (range, ok("2\n")),
            (
                "rev-list --parents -n 1 refs/heads/main",
                ok(&format!("{MERGED} {PRE} {other}\n")),
            ),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("main has 2 commit(s) that are not on origin; push or undo them yourself")
    );
    assert!(!o.merged);
}
