use super::read::{cap, read_diff, read_status, Failure, MAX_DIFF_BYTES, MAX_DIFF_LINES};
use crate::control::launcher::{FakeLauncher, LaunchError, LaunchOutput};
use crate::sessions::testutil::TempDir;
use std::time::Duration;

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

fn args_of(launcher: &FakeLauncher) -> Vec<Vec<String>> {
    launcher.commands().into_iter().map(|c| c.argv).collect()
}

fn has(argv: &[String], word: &str) -> bool {
    argv.iter().any(|a| a == word)
}

#[test]
fn status_runs_the_read_only_commands_with_a_timeout_and_no_prompts() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid abc1234def",
        "# branch.head main",
        "1 .M N... 100644 100644 100644 a b src/a.rs",
    ]))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["3\t1\tsrc/a.rs"]))));
    let status = read_status("/w/app", &fake).unwrap();
    assert_eq!(status.changes[0].added, Some(3));
    assert_eq!(status.changes[0].removed, Some(1));
    let calls = args_of(&fake);
    assert_eq!(calls.len(), 2);
    for argv in &calls {
        assert_eq!(argv[0], "git");
        assert!(has(argv, "--no-optional-locks"));
        assert!(has(argv, "core.fsmonitor=false"));
        assert_eq!(
            argv[argv.iter().position(|a| a == "-C").unwrap() + 1],
            "/w/app"
        );
    }
    assert!(has(&calls[0], "--porcelain=v2") && has(&calls[0], "--branch") && has(&calls[0], "-z"));
    assert!(has(&calls[1], "--numstat") && has(&calls[1], "-z"));
    assert!(has(&calls[1], "--no-color") && has(&calls[1], "--no-ext-diff"));
    assert!(has(&calls[1], "HEAD"));
    for command in fake.commands() {
        assert_eq!(
            command.env.get("GIT_TERMINAL_PROMPT"),
            Some(&Some("0".to_owned()))
        );
        assert_eq!(command.env.get("GIT_PAGER"), Some(&Some("cat".to_owned())));
    }
    for call in fake.calls() {
        if let crate::control::launcher::Call::Run(_, timeout) = call {
            assert_eq!(timeout, Duration::from_secs(3));
        }
    }
}

#[test]
fn a_clean_tree_runs_one_command_and_a_fresh_repo_counts_the_index() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&z(&["# branch.head main"]))));
    assert!(read_status("/w", &fake).unwrap().changes.is_empty());
    assert_eq!(fake.commands().len(), 1);

    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid (initial)",
        "# branch.head main",
        "? a.txt",
    ]))));
    fake.push_output(Ok(LaunchOutput::ok("")));
    read_status("/w", &fake).unwrap();
    let calls = args_of(&fake);
    assert!(has(&calls[1], "--cached") && !has(&calls[1], "HEAD"));
}

#[test]
fn a_failed_numstat_keeps_the_files_without_counts() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&z(&["? a.txt"]))));
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
    let status = read_status("/w", &fake).unwrap();
    assert_eq!(status.changes.len(), 1);
    assert_eq!(status.changes[0].added, None);
}

#[test]
fn not_a_repository_timeouts_and_other_failures_are_told_apart() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput {
        code: 128,
        output: String::new(),
        stderr: "fatal: not a git repository (or any of the parent directories): .git\n".into(),
    }));
    assert_eq!(read_status("/w", &fake), Err(Failure::NotRepo));

    let fake = FakeLauncher::new();
    fake.push_output(Err(LaunchError::Timeout(3)));
    assert!(matches!(read_status("/w", &fake), Err(Failure::Failed(_))));

    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput {
        code: 128,
        output: String::new(),
        stderr: "fatal: bad object HEAD\n".into(),
    }));
    assert!(matches!(read_status("/w", &fake), Err(Failure::Failed(_))));
}

#[test]
fn a_diff_asks_for_the_file_alone_and_cuts_a_long_answer() {
    let t = TempDir::new("git-diff-fake");
    t.write("src/a.rs", "x");
    let root = t.root().to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&format!("{root}\n"))));
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.oid abc1234def",
        "# branch.head main",
        "1 .M N... 100644 100644 100644 a b src/a.rs",
    ]))));
    let long: String = (0..MAX_DIFF_LINES + 50)
        .map(|n| format!("+line {n}\n"))
        .collect();
    fake.push_output(Ok(LaunchOutput::ok(&long)));
    let diff = read_diff(&root, "src/a.rs", &fake).unwrap();
    assert!(diff.truncated && !diff.untracked);
    assert_eq!(diff.text.lines().count(), MAX_DIFF_LINES);
    let calls = args_of(&fake);
    assert_eq!(calls.len(), 3);
    let last = &calls[2];
    for flag in ["--no-color", "--no-ext-diff", "--no-textconv", "HEAD", "--"] {
        assert!(has(last, flag), "{flag}");
    }
    assert_eq!(last.last().unwrap(), ":(literal)src/a.rs");
}

#[test]
fn an_untracked_file_is_read_with_no_index_and_exit_one_is_normal() {
    let t = TempDir::new("git-diff-untracked");
    t.write("new.txt", "hello\n");
    let root = t.root().to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    fake.push_output(Ok(LaunchOutput::ok(&z(&[
        "# branch.head main",
        "? new.txt",
    ]))));
    fake.push_output(Ok(LaunchOutput::failed(
        1,
        "diff --git a/new.txt b/new.txt\n+hello\n",
    )));
    let diff = read_diff(&root, "new.txt", &fake).unwrap();
    assert!(diff.untracked && !diff.truncated);
    assert!(diff.text.contains("+hello"));
    assert!(has(&args_of(&fake)[2], "--no-index"));
}

#[test]
fn a_file_too_big_to_diff_says_truncated_and_runs_nothing_more() {
    let t = TempDir::new("git-diff-big");
    t.write("big.bin", &"x".repeat(2 * 1024 * 1024 + 10));
    let root = t.root().to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    fake.push_output(Ok(LaunchOutput::ok(&z(&["? big.bin"]))));
    let diff = read_diff(&root, "big.bin", &fake).unwrap();
    assert!(diff.truncated && diff.text.is_empty());
    assert_eq!(fake.commands().len(), 2);
}

#[test]
fn a_path_that_escapes_is_refused_before_anything_runs() {
    let fake = FakeLauncher::new();
    for path in ["../x", "/etc/passwd", "a/../../b", ""] {
        assert!(
            matches!(read_diff("/w", path, &fake), Err(Failure::Refused(_))),
            "{path:?}"
        );
    }
    assert!(fake.commands().is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_that_leaves_the_repository_is_refused() {
    let t = TempDir::new("git-diff-link");
    t.write("repo/a.txt", "x");
    t.write("outside/secret.txt", "x");
    std::os::unix::fs::symlink(t.path("outside"), t.path("repo/dir")).unwrap();
    let root = t.path("repo").to_string_lossy().into_owned();
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(&root)));
    let got = read_diff(&root, "dir/secret.txt", &fake);
    assert!(matches!(got, Err(Failure::Refused(_))), "{got:?}");
    assert_eq!(fake.commands().len(), 1, "only rev-parse ran");
}

#[test]
fn the_cap_cuts_on_lines_bytes_and_characters() {
    assert_eq!(cap("a\nb\n", 5, 100), ("a\nb\n".to_owned(), false));
    assert_eq!(cap("a\nb\nc\n", 2, 100), ("a\nb\n".to_owned(), true));
    assert_eq!(cap("aa\nbb\ncc\n", 10, 7), ("aa\nbb\n".to_owned(), true));
    let wide = "\u{65e5}".repeat(10);
    let (cut, truncated) = cap(&wide, 10, 7);
    assert!(truncated);
    assert_eq!(cut, "\u{65e5}\u{65e5}", "a whole character, never a half");
    let (big, truncated) = cap(&"x\n".repeat(MAX_DIFF_BYTES), usize::MAX, MAX_DIFF_BYTES);
    assert!(truncated && big.len() <= MAX_DIFF_BYTES);
}

#[cfg(unix)]
mod real {
    use super::*;
    use crate::control::RealLauncher;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false", "-C"])
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn launcher() -> RealLauncher {
        RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"))
    }

    fn repo(tag: &str) -> Option<TempDir> {
        let t = TempDir::new(tag);
        let ok = git(t.root(), &["init", "-q", "-b", "main"]);
        ok.then_some(t)
    }

    #[test]
    fn a_real_repository_reads_back_its_changes_and_diffs() {
        let Some(t) = repo("git-real") else { return };
        t.write("a.txt", "one\ntwo\n");
        t.write("gone.txt", "bye\n");
        assert!(git(t.root(), &["add", "."]));
        assert!(git(t.root(), &["commit", "-q", "-m", "first"]));
        t.write("a.txt", "one\nthree\nfour\n");
        t.write("b.txt", "hello\n");
        t.write("c.txt", "staged\n");
        std::fs::remove_file(t.path("gone.txt")).unwrap();
        assert!(git(t.root(), &["add", "c.txt"]));
        let cwd = t.root().to_string_lossy().into_owned();
        let status = read_status(&cwd, &launcher()).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream, None);
        let rows: Vec<(&str, &str)> = status
            .changes
            .iter()
            .map(|c| (c.status.as_str(), c.path.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("M", "a.txt"),
                ("A", "c.txt"),
                ("D", "gone.txt"),
                ("?", "b.txt")
            ]
        );
        let a = &status.changes[0];
        assert_eq!((a.added, a.removed), (Some(2), Some(1)));
        assert_eq!(status.changes[1].added, Some(1));

        let diff = read_diff(&cwd, "a.txt", &launcher()).unwrap();
        assert!(
            diff.text.contains("-two") && diff.text.contains("+three"),
            "{}",
            diff.text
        );
        assert!(!diff.untracked);
        let fresh = read_diff(&cwd, "b.txt", &launcher()).unwrap();
        assert!(
            fresh.untracked && fresh.text.contains("+hello"),
            "{}",
            fresh.text
        );
        let removed = read_diff(&cwd, "gone.txt", &launcher()).unwrap();
        assert!(removed.text.contains("-bye"));

        t.write("sub/deep.txt", "x\n");
        let below = t.path("sub").to_string_lossy().into_owned();
        let diff = read_diff(&below, "sub/deep.txt", &launcher()).unwrap();
        assert!(diff.untracked && diff.text.contains("+x"));
    }

    #[test]
    fn a_repository_with_no_commit_yet_still_reads() {
        let Some(t) = repo("git-real-initial") else {
            return;
        };
        t.write("a.txt", "one\n");
        assert!(git(t.root(), &["add", "a.txt"]));
        let cwd = t.root().to_string_lossy().into_owned();
        let status = read_status(&cwd, &launcher()).unwrap();
        assert!(status.initial);
        assert_eq!(status.changes[0].added, Some(1));
        let diff = read_diff(&cwd, "a.txt", &launcher()).unwrap();
        assert!(diff.text.contains("+one"));
    }

    #[test]
    fn a_folder_outside_any_repository_is_not_one() {
        let t = TempDir::new("git-real-none");
        let cwd = t.root().to_string_lossy().into_owned();
        assert_eq!(read_status(&cwd, &launcher()), Err(Failure::NotRepo));
    }

    #[test]
    fn a_tracked_link_to_outside_is_not_followed() {
        let Some(t) = repo("git-real-link") else {
            return;
        };
        t.write("outside/secret.txt", "SECRET\n");
        t.write("repo/a.txt", "x\n");
        let repo_dir = t.path("repo");
        assert!(git(&repo_dir, &["init", "-q", "-b", "main"]));
        std::os::unix::fs::symlink(t.path("outside"), repo_dir.join("dir")).unwrap();
        let cwd = repo_dir.to_string_lossy().into_owned();
        let got = read_diff(&cwd, "dir/secret.txt", &launcher());
        assert!(matches!(got, Err(Failure::Refused(_))), "{got:?}");
    }
}
