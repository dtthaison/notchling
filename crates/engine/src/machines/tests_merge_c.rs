use std::collections::BTreeMap;

use super::fakes_jobs::{bad, bad_code, ok};
use super::merge::{MergeOutcome, MergePlan};
use super::tests_merge::{any_line, entry, happy, merge, plan, t, MERGED, PRE, SHA, TARGET};

const REF: &str = "refs/machines/m1/7";

#[test]
fn conflict_aborts_and_labels_needs_you() {
    let runner = happy(
        vec![
            (
                "merge --no-ff",
                bad_code(
                    "CONFLICT (content): Merge conflict in a.swift",
                    1,
                    "Automatic merge failed",
                ),
            ),
            ("diff --name-only", ok("a.swift\nb/c.swift\n")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(o.conflict_files, vec!["a.swift", "b/c.swift"]);
    assert!(!o.merged);
    let calls = runner.calls();
    let lines = runner.lines();
    let n = calls.len();
    assert_eq!(
        lines[n - 5..n - 2],
        [
            t("rev-parse -q --verify MERGE_HEAD"),
            t("diff --name-only --diff-filter=U"),
            t("merge --abort"),
        ]
    );
    let (comment, edit) = (&calls[n - 2], &calls[n - 1]);
    assert_eq!(comment[1..6], ["issue", "comment", "7", "-R", "acme/app"]);
    assert!(comment
        .last()
        .is_some_and(|b| b.starts_with("notchling: ") && b.contains("a.swift, b/c.swift")));
    assert_eq!(
        edit[1..],
        [
            "issue",
            "edit",
            "7",
            "-R",
            "acme/app",
            "--remove-label",
            "sd:ready",
            "--add-label",
            "sd:needs-you"
        ]
    );
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.contains("-lc")));
}

#[test]
fn merge_failure_without_conflict_labels_nothing() {
    let runner = happy(
        vec![(
            "merge --no-ff",
            bad("fatal: refusing to merge unrelated histories"),
        )],
        None,
    );
    let o = merge(&entry(), &runner);
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("unrelated histories")));
    assert!(o.conflict_files.is_empty());
    assert!(!any_line(&runner, |l| l.starts_with("gh ")));
}

#[test]
fn a_merge_that_never_started_reports_its_own_error_and_does_not_abort() {
    let runner = happy(
        vec![
            (
                "merge --no-ff",
                bad("fatal: refusing to merge unrelated histories"),
            ),
            ("--verify MERGE_HEAD", bad_code("", 1, "")),
            (
                "merge --abort",
                bad_code(
                    "fatal: There is no merge to abort (MERGE_HEAD missing).",
                    128,
                    "",
                ),
            ),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    assert_eq!(
        o.error.as_deref(),
        Some("merge failed: fatal: refusing to merge unrelated histories")
    );
    assert!(!any_line(&runner, |l| l.contains("merge --abort")
        || l.contains("diff --name-only")
        || l.starts_with("gh ")));
    assert_eq!(
        runner.lines().last(),
        Some(&t("rev-parse -q --verify MERGE_HEAD"))
    );
}

#[test]
fn nothing_to_merge_stops_before_the_check() {
    let runner = happy(
        vec![],
        Some(vec![("rev-parse HEAD", vec![ok(&format!("{PRE}\n"))])]),
    );
    let o = merge(&entry(), &runner);
    assert!(!o.merged);
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("already in main")));
    assert!(!any_line(&runner, |l| l.contains(" reset ")
        || l.contains(" push ")
        || l.starts_with("zsh")));
}

#[test]
fn check_failure_resets_and_keeps_ready() {
    let runner = happy(
        vec![(
            "-lc",
            bad_code("error: 2 tests failed", 1, "building...\nFAIL FooTests"),
        )],
        None,
    );
    let o = merge(&entry(), &runner);
    assert!(!o.checked);
    assert!(!o.merged);
    assert!(!o.pushed);
    assert!(!o.can_retry_push());
    assert!(!o.can_undo());
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("2 tests failed")));
    let lines = runner.lines();
    assert_eq!(
        lines[lines.len() - 3..],
        [
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD"),
            t(&format!("reset --keep {PRE}"))
        ]
    );
    assert!(!any_line(&runner, |l| l.contains("ORIG_HEAD") || l.contains("--hard")));
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.starts_with("gh ")));
    let recs = runner.recs();
    let check = recs
        .iter()
        .find(|r| r.line().contains(" -lc "))
        .expect("the check ran");
    assert_eq!(check.cwd.as_deref().and_then(|p| p.to_str()), Some(TARGET));
    assert_eq!(check.timeout.as_secs(), 1800);
}

#[test]
fn failed_check_whose_reset_fails_is_a_hard_error_with_no_retry() {
    let runner = happy(
        vec![
            ("-lc", bad("error: 2 tests failed")),
            ("reset --keep", bad("fatal: Unable to create index.lock")),
        ],
        None,
    );
    let o = merge(&entry(), &runner);
    let want = format!("check failed and main could not be put back; fix it by hand in {TARGET}");
    assert!(
        o.error.as_deref().is_some_and(|e| e.starts_with(&want)),
        "{:?}",
        o.error
    );
    assert!(o
        .error
        .as_deref()
        .is_some_and(|e| e.contains("2 tests failed")));
    assert!(o.merged);
    assert!(!o.checked);
    assert!(!o.can_retry_push());
    assert!(!o.pushed);
    assert!(!any_line(&runner, |l| l.contains(" push ") || l.starts_with("gh ")));
}

#[test]
fn push_failure_keeps_issue_open() {
    let runner = happy(vec![("push origin", bad("remote: rejected"))], None);
    let o = merge(&entry(), &runner);
    assert!(o.merged && o.checked);
    assert!(!o.pushed && !o.closed);
    assert_eq!(o.commit.as_deref(), Some(MERGED));
    assert!(o.can_retry_push());
    assert!(o.can_undo());
    assert!(o.error.as_deref().is_some_and(|e| e.contains("rejected")));
    assert!(!any_line(&runner, |l| l.starts_with("gh ") || l.contains(" reset ")));
}

#[test]
fn happy_path_order() {
    let runner = happy(vec![], None);
    let o = merge(&entry(), &runner);
    assert_eq!(
        o,
        MergeOutcome {
            fetched: true,
            merged: true,
            checked: true,
            pushed: true,
            closed: true,
            commit: Some(MERGED.into()),
            error: None,
            conflict_files: vec![],
        }
    );
    let ahead = "refs/remotes/origin/main..refs/heads/main";
    assert_eq!(
        runner.lines(),
        vec![
            "git -C /Users/me/code/app worktree list --porcelain".to_string(),
            t("check-ref-format --branch sd/issue-7"),
            t("status --porcelain"),
            t("fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main"),
            t("merge --ff-only refs/remotes/origin/main"),
            t(&format!("rev-list --count {ahead}")),
            t(&format!(
                "fetch --no-tags ssh://agent@m1.tail/work/app-7 +refs/heads/sd/issue-7:{REF}"
            )),
            t(&format!("rev-parse {REF}")),
            t("rev-parse HEAD"),
            t(&format!(
                "merge --no-ff -m Merge sd/issue-7 (acme/app#7) {SHA}"
            )),
            t("rev-parse HEAD"),
            "zsh -lc just check".to_string(),
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD"),
            t("status --porcelain"),
            t("push origin refs/heads/main:refs/heads/main"),
            format!("gh issue close 7 -R acme/app --comment notchling: merged in {MERGED}"),
            "gh issue edit 7 -R acme/app --remove-label sd:ready".to_string(),
        ]
    );
}

#[test]
fn no_ref_is_a_short_name_and_every_fetch_says_no_tags() {
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    let fetches: Vec<_> = runner
        .calls()
        .into_iter()
        .filter(|c| c.contains(&"fetch".to_string()))
        .collect();
    assert_eq!(fetches.len(), 2);
    for f in &fetches {
        assert!(f.contains(&"--no-tags".to_string()), "{f:?}");
    }
    for argv in runner.calls() {
        if argv[0].ends_with("/git") {
            for word in &argv {
                assert!(
                    !["main", "origin/main"].contains(&word.as_str()),
                    "short ref '{word}' in {argv:?}"
                );
            }
        }
    }
    let pushes: Vec<_> = runner
        .calls()
        .into_iter()
        .filter(|c| c.contains(&"push".to_string()))
        .collect();
    assert_eq!(
        pushes,
        vec![[
            "/usr/bin/git",
            "-C",
            TARGET,
            "push",
            "origin",
            "refs/heads/main:refs/heads/main"
        ]
        .map(String::from)
        .to_vec()]
    );
}

#[test]
fn only_main_is_pushed_and_never_forced() {
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    for argv in runner.calls() {
        if argv.contains(&"-lc".to_string()) || argv.contains(&"fetch".to_string()) {
            continue;
        }
        assert!(!argv.iter().any(|a| {
            a == "--force"
                || a == "-f"
                || a.starts_with("--force-")
                || a.starts_with("+refs/heads")
                || a.starts_with("+main")
        }));
    }
}

#[test]
fn git_calls_use_full_paths_and_timeouts() {
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    for r in runner.recs() {
        assert!(
            r.argv[0].starts_with('/'),
            "{} is not a full path",
            r.argv[0]
        );
        if r.argv[0] == "/usr/bin/git" && r.argv.contains(&"fetch".to_string()) {
            assert_eq!(r.timeout.as_secs(), 300);
        } else if r.argv[0] == "/usr/bin/git" {
            assert_eq!(r.timeout.as_secs(), 60);
        }
    }
}

#[test]
fn git_and_gh_come_from_the_tool_lookup() {
    let here = MergePlan::from_process(std::path::Path::new("/data/merge"));
    assert_eq!(here.merge_root, "/data/merge");
    assert_eq!(here.git, super::shell::tool_path("git"));
    assert_eq!(here.gh, super::shell::tool_path("gh"));
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    assert!(runner.recs().iter().any(|r| r.exe() == "gh"));
    for r in runner.recs() {
        match r.exe() {
            "git" => assert_eq!(r.argv[0], plan().git),
            "gh" => assert_eq!(r.argv[0], plan().gh),
            _ => {}
        }
    }
}

#[test]
fn check_runs_in_the_login_shell_with_tool_directories_first_on_path() {
    let runner = happy(vec![], None);
    let _ = merge(&entry(), &runner);
    let recs = runner.recs();
    let i = recs
        .iter()
        .position(|r| r.argv.contains(&"-lc".to_string()))
        .expect("the check ran");
    let (argv, env) = plan().check_invocation("just check");
    assert_eq!(recs[i].argv, argv);
    assert_eq!(recs[i].argv[1..], ["-lc", "just check"]);
    assert_eq!(recs[i].env, env);
    let path = &recs[i].env[0].1;
    assert!(path.starts_with(
        "/Users/me/.cargo/bin:/Users/me/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:"
    ));
    assert!(path.ends_with("/usr/bin:/bin"));
    assert_eq!(
        recs[i].cwd.as_deref().and_then(|p| p.to_str()),
        Some(TARGET)
    );
    let fetch_at = recs
        .iter()
        .position(|r| {
            r.argv.contains(&"fetch".to_string()) && r.argv.iter().any(|a| a.starts_with("ssh://"))
        })
        .expect("the machine fetch");
    let with_env: Vec<usize> = recs
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.env.is_empty())
        .map(|(n, _)| n)
        .collect();
    assert_eq!(with_env, vec![fetch_at, i]);
}

#[test]
fn check_invocation_uses_the_users_shell_when_it_is_one_we_can_run() {
    let env = BTreeMap::from([
        ("SHELL".to_string(), "/bin/bash".to_string()),
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
    ]);
    let (argv, vars) =
        MergePlan::check_invocation_with("just check-affected", &env, "/Users/me", &|_| true);
    assert_eq!(argv, ["/bin/bash", "-lc", "just check-affected"]);
    assert_eq!(
        vars,
        vec![(
            "PATH".to_string(),
            "/Users/me/.cargo/bin:/Users/me/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_string()
        )]
    );
}

#[test]
fn check_invocation_falls_back_to_zsh_without_a_usable_shell() {
    let cases = [
        vec![],
        vec![("SHELL", "")],
        vec![("SHELL", "zsh")],
        vec![("SHELL", "/nonexistent/shell")],
    ];
    for case in cases {
        let env: BTreeMap<String, String> = case
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let (argv, vars) =
            MergePlan::check_invocation_with("c", &env, "/Users/me", &|p| p == "/bin/zsh");
        assert_eq!(argv, ["/bin/zsh", "-lc", "c"], "{case:?}");
        assert_eq!(
            vars[0].1,
            "/Users/me/.cargo/bin:/Users/me/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }
}
