use super::branch::{read_branch_stat, read_branches};
use super::branch_file::read_branch_diff;
use super::read::Failure;
use crate::control::launcher::{FakeLauncher, LaunchOutput};
use crate::sessions::testutil::TempDir;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

fn argv(fake: &FakeLauncher) -> Vec<Vec<String>> {
    fake.commands().into_iter().map(|c| c.argv).collect()
}

fn has(argv: &[String], word: &str) -> bool {
    argv.iter().any(|a| a == word)
}

fn no_filters(fake: &FakeLauncher) {
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
}

#[test]
fn a_stat_runs_only_read_commands_against_the_merge_base() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid abc1234def",
        "# branch.head feature",
        "1 .M N... 100644 100644 100644 a b a.rs",
    ]))));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    fake.push_output(Ok(LaunchOutput::ok(&format!("{SHA}\n"))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["M", "a.rs", "A", "n.rs"]))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["3\t1\ta.rs", "5\t0\tn.rs"]))));
    let stat = read_branch_stat("/w/app", None, &fake).unwrap();
    assert_eq!(stat.branch.as_deref(), Some("feature"));
    assert_eq!(stat.base.as_deref(), Some("main"));
    assert_eq!(stat.merge_base.as_deref(), Some("0123456"));
    assert_eq!((stat.added, stat.removed, stat.file_count), (8, 1, 2));
    assert_eq!(stat.uncommitted_files, 1);
    assert!(stat.files[0].uncommitted && !stat.files[1].uncommitted);
    let calls = argv(&fake);
    assert_eq!(calls.len(), 6);
    assert!(has(&calls[2], "symbolic-ref") && has(&calls[2], "refs/remotes/origin/HEAD"));
    assert!(has(&calls[3], "merge-base") && has(&calls[3], "main") && has(&calls[3], "HEAD"));
    for (at, mode) in [(4, "--name-status"), (5, "--numstat")] {
        let call = &calls[at];
        assert!(has(call, "diff") && has(call, mode) && has(call, "-z") && has(call, "-M"));
        assert!(has(call, "--no-ext-diff") && has(call, "--no-textconv"));
        assert!(has(call, SHA) && has(call, "--"));
    }
    for call in &calls {
        assert_eq!(call[0], "git");
        assert!(has(call, "--no-optional-locks"));
        assert!(has(call, "core.hooksPath=/dev/null"));
        for write in [
            "add",
            "commit",
            "checkout",
            "reset",
            "fetch",
            "push",
            "update-ref",
        ] {
            assert!(!has(call, write), "{write}");
        }
    }
}

#[test]
fn the_remote_default_wins_and_an_asked_base_is_used_as_given() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head f"]))));
    fake.push_output(Ok(LaunchOutput::ok("origin/trunk\n")));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    let stat = read_branch_stat("/w", None, &fake).unwrap();
    assert_eq!(stat.base.as_deref(), Some("origin/trunk"));
    assert!(has(&argv(&fake)[3], "origin/trunk"));

    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head f"]))));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    let stat = read_branch_stat("/w", Some("release/1"), &fake).unwrap();
    assert_eq!(stat.base.as_deref(), Some("release/1"));
    assert_eq!(argv(&fake).len(), 5, "no default is looked up");
}

#[test]
fn a_repo_with_no_commit_or_no_base_answers_without_one() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid (initial)",
        "# branch.head main",
        "? a.txt",
    ]))));
    let stat = read_branch_stat("/w", None, &fake).unwrap();
    assert_eq!((stat.base, stat.files.len()), (None, 0));
    assert_eq!(argv(&fake).len(), 2);

    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head odd"]))));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    let stat = read_branch_stat("/w", None, &fake).unwrap();
    assert_eq!(stat.base, None);
    assert_eq!(stat.branch.as_deref(), Some("odd"));
}

#[test]
fn an_unknown_base_fails_and_an_odd_one_is_refused_before_anything_runs() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head f"]))));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    assert_eq!(
        read_branch_stat("/w", Some("nope"), &fake),
        Err(Failure::Failed("no such branch".to_owned()))
    );
    for bad in ["--output=/tmp/x", "a b", "-x", "a..b", ""] {
        let fake = FakeLauncher::new();
        assert!(matches!(
            read_branch_stat("/w", Some(bad), &fake),
            Err(Failure::Refused(_))
        ));
        assert!(matches!(
            read_branch_diff("/w", bad, "a.rs", None, &fake),
            Err(Failure::Refused(_))
        ));
        assert!(fake.commands().is_empty(), "{bad}");
    }
}

#[test]
fn not_a_repository_is_told_apart() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput {
        code: 128,
        output: String::new(),
        stderr: "fatal: not a git repository\n".into(),
    }));
    fake.push_output(Ok(LaunchOutput::failed(128, "")));
    assert_eq!(read_branch_stat("/w", None, &fake), Err(Failure::NotRepo));
}

#[test]
fn untracked_files_are_counted_by_reading_them_and_a_binary_or_a_link_is_not() {
    let t = TempDir::new("branch-untracked");
    t.write("repo/three.txt", "a\nb\nc");
    t.write("repo/two.txt", "a\nb\n");
    t.write("repo/bin.dat", "a\0b\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink(t.path("repo/two.txt"), t.path("repo/link.txt")).unwrap();
    let root = t.path("repo").to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    no_filters(&fake);
    let mut records = vec!["# branch.head f", "? three.txt", "? two.txt", "? bin.dat"];
    #[cfg(unix)]
    records.push("? link.txt");
    fake.push_output(Ok(LaunchOutput::ok(&z(&records))));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    let stat = read_branch_stat("/w", Some("main"), &fake).unwrap();
    let find = |name: &str| stat.files.iter().find(|f| f.path == name).unwrap();
    assert_eq!(
        (find("three.txt").added, find("three.txt").removed),
        (Some(3), Some(0))
    );
    assert_eq!(find("two.txt").added, Some(2));
    assert_eq!(find("bin.dat").added, None);
    #[cfg(unix)]
    assert_eq!(find("link.txt").added, None);
    assert!(stat.files.iter().all(|f| f.status == "?" && f.uncommitted));
    assert_eq!(stat.uncommitted_files, stat.file_count);
}

#[test]
fn the_base_menu_lists_local_branches_and_resolves_a_default() {
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok("  main\n* feature\n  dev\n")));
    fake.push_output(Ok(LaunchOutput::ok("origin/main\n")));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    let got = read_branches("/w", &fake).unwrap();
    assert_eq!(got.current.as_deref(), Some("feature"));
    assert_eq!(got.local, ["main", "feature", "dev"]);
    assert_eq!(got.remote_default.as_deref(), Some("origin/main"));
    assert_eq!(got.default.as_deref(), Some("origin/main"));
    let calls = argv(&fake);
    assert!(has(&calls[1], "for-each-ref") && has(&calls[1], "refs/heads"));
    assert!(has(&calls[3], "rev-parse") && has(&calls[3], "origin/main^{commit}"));

    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok("* feature\n  master\n")));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    let got = read_branches("/w", &fake).unwrap();
    assert_eq!(
        (got.remote_default, got.default.as_deref()),
        (None, Some("master"))
    );

    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok("* odd\n")));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    assert_eq!(read_branches("/w", &fake).unwrap().default, None);
}

#[test]
fn a_file_patch_diffs_against_the_merge_base_and_follows_a_rename() {
    let t = TempDir::new("branch-file");
    t.write("repo/moved.rs", "x");
    let root = t.path("repo").to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    no_filters(&fake);
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    fake.push_output(Ok(LaunchOutput::ok(SHA)));
    fake.push_output(Ok(LaunchOutput::ok(
        "diff --git a/old.rs b/moved.rs\n-x\n+y\n",
    )));
    let diff = read_branch_diff("/w", "main", "moved.rs", Some("old.rs"), &fake).unwrap();
    assert_eq!(diff.path, "moved.rs");
    assert!(diff.text.contains("+y") && !diff.truncated && !diff.untracked);
    let calls = argv(&fake);
    let last = calls.last().unwrap();
    assert!(has(last, "diff") && has(last, SHA) && has(last, "-M"));
    assert!(has(last, ":(literal)moved.rs") && has(last, ":(literal)old.rs"));
    assert!(has(last, "--no-ext-diff") && has(last, "--no-textconv"));
}

#[test]
fn an_empty_patch_is_an_untracked_file_or_not_a_changed_file() {
    let t = TempDir::new("branch-file-empty");
    t.write("repo/fresh.rs", "x");
    let root = t.path("repo").to_string_lossy().into_owned();
    let rig = |status: &[&str]| {
        let fake = FakeLauncher::new();
        no_filters(&fake);
        fake.push_output(Ok(LaunchOutput::ok(&root)));
        fake.push_output(Ok(LaunchOutput::ok(SHA)));
        fake.push_output(Ok(LaunchOutput::ok("")));
        fake.push_output(Ok(LaunchOutput::ok(&z(status))));
        fake
    };
    let fake = rig(&["# branch.head f"]);
    assert_eq!(
        read_branch_diff("/w", "main", "clean.rs", None, &fake),
        Err(Failure::Refused("not a changed file".to_owned()))
    );
    let fake = rig(&["# branch.head f", "? fresh.rs"]);
    fake.push_output(Ok(LaunchOutput::ok(
        "diff --git a/fresh.rs b/fresh.rs\n+x\n",
    )));
    let diff = read_branch_diff("/w", "main", "fresh.rs", None, &fake).unwrap();
    assert!(diff.untracked && diff.text.contains("+x"));
    assert!(has(argv(&fake).last().unwrap(), "--no-index"));
}

#[test]
fn a_file_path_that_leaves_the_repository_is_refused_before_anything_runs() {
    for (path, from) in [
        ("../x", None),
        ("/etc/passwd", None),
        ("a.rs", Some("../y")),
    ] {
        let fake = FakeLauncher::new();
        assert!(matches!(
            read_branch_diff("/w", "main", path, from, &fake),
            Err(Failure::Refused(_))
        ));
        assert!(fake.commands().is_empty());
    }
}
