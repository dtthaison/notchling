use super::sandbox::{profile, root_owned_chain, Owner, SandboxedLauncher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[test]
fn the_profile_denies_every_exec_and_allows_only_the_named_programs() {
    let text = profile(
        &["/usr/bin/git".to_owned(), "/odd \"dir\"/git".to_owned()],
        &["/Library/Developer/CommandLineTools/usr/libexec/git-core".to_owned()],
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "(version 1)");
    assert_eq!(lines[1], "(allow default)");
    assert_eq!(lines[2], "(deny process-exec*)");
    assert!(text.contains("(literal \"/usr/bin/git\")"));
    assert!(text.contains("(literal \"/odd \\\"dir\\\"/git\")"));
    assert!(text.contains("(subpath \"/Library/Developer/CommandLineTools/usr/libexec/git-core\")"));
    let deny = text.find("(deny process-exec*)").unwrap();
    let allow = text.find("(allow process-exec").unwrap();
    assert!(deny < allow, "the allow comes after the deny, so it wins");
}

fn tree(entries: &[(&str, u32, u32)]) -> HashMap<PathBuf, Owner> {
    entries
        .iter()
        .map(|(path, uid, mode)| {
            (
                PathBuf::from(path),
                Owner {
                    uid: *uid,
                    mode: *mode,
                },
            )
        })
        .collect()
}

fn passes(map: &HashMap<PathBuf, Owner>, path: &str) -> bool {
    root_owned_chain(Path::new(path), &|p| map.get(p).copied())
}

#[test]
fn a_path_passes_only_when_it_and_every_parent_is_root_owned_and_not_writable_by_others() {
    let good = tree(&[
        ("/", 0, 0o40755),
        ("/usr", 0, 0o40755),
        ("/usr/bin", 0, 0o40755),
        ("/usr/bin/git", 0, 0o100755),
    ]);
    assert!(passes(&good, "/usr/bin/git"));

    let user_file = tree(&[
        ("/", 0, 0o40755),
        ("/opt", 0, 0o40755),
        ("/opt/git", 501, 0o100755),
    ]);
    assert!(!passes(&user_file, "/opt/git"));

    let user_parent = tree(&[
        ("/", 0, 0o40755),
        ("/opt", 501, 0o40755),
        ("/opt/git", 0, 0o100755),
    ]);
    assert!(!passes(&user_parent, "/opt/git"));

    let admin_parent = tree(&[
        ("/", 0, 0o40755),
        ("/Applications", 0, 0o40775),
        ("/Applications/git", 0, 0o100755),
    ]);
    assert!(!passes(&admin_parent, "/Applications/git"));

    let world_file = tree(&[("/", 0, 0o40755), ("/git", 0, 0o100757)]);
    assert!(!passes(&world_file, "/git"));

    let sticky_tmp = tree(&[
        ("/", 0, 0o40755),
        ("/tmp", 0, 0o41777),
        ("/tmp/git", 0, 0o100755),
    ]);
    assert!(!passes(&sticky_tmp, "/tmp/git"));
}

#[test]
fn a_missing_entry_a_user_owned_root_or_a_relative_path_is_refused() {
    let hole = tree(&[("/", 0, 0o40755), ("/usr/bin/git", 0, 0o100755)]);
    assert!(!passes(&hole, "/usr/bin/git"));
    let user_root = tree(&[("/", 501, 0o40755), ("/git", 0, 0o100755)]);
    assert!(!passes(&user_root, "/git"));
    let good = tree(&[("/", 0, 0o40755), ("git", 0, 0o100755)]);
    assert!(!passes(&good, "git"));
}

#[test]
fn a_profile_with_nothing_allowed_still_denies_every_exec() {
    let text = profile(&[], &[]);
    assert!(text.contains("(deny process-exec*)") && !text.contains("(allow process-exec"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn off_macos_nothing_runs_and_the_answer_is_unsupported() {
    use crate::control::launcher::{FakeLauncher, LaunchError, Launcher};
    use crate::control::Command;
    use std::sync::Arc;
    use std::time::Duration;
    let fake = Arc::new(FakeLauncher::new());
    let sandboxed = SandboxedLauncher::new(fake.clone());
    let got = sandboxed.run(&Command::new(&["git", "status"]), Duration::from_secs(1));
    assert_eq!(got, Err(LaunchError::Unsupported));
    assert!(fake.commands().is_empty());
    assert_eq!(
        super::read::read_status("/w", &sandboxed),
        Err(super::read::Failure::Unsupported)
    );
}

#[cfg(target_os = "macos")]
mod real {
    use super::*;
    use crate::control::launcher::{LaunchError, Launcher};
    use crate::control::{Command, RealLauncher};
    use crate::git::read::{read_diff, read_status, Failure};
    use crate::sessions::testutil::TempDir;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    fn plain(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false", "-C"])
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn must(dir: &Path, args: &[&str]) {
        assert!(plain(dir, args), "git {args:?} failed");
    }

    fn sandboxed() -> SandboxedLauncher {
        SandboxedLauncher::waiting(
            Arc::new(RealLauncher::with_claude(PathBuf::from(
                "/nonexistent/claude",
            ))),
            Duration::from_secs(60),
        )
    }

    struct Rig {
        t: TempDir,
        repo: PathBuf,
        marker: PathBuf,
    }

    fn rig(tag: &str) -> Rig {
        let t = TempDir::new(tag);
        let repo = t.path("repo");
        std::fs::create_dir_all(&repo).unwrap();
        must(&repo, &["init", "-q", "-b", "main"]);
        t.write("repo/a.txt", "one\n");
        t.write("repo/c.txt", "one\n");
        must(&repo, &["add", "."]);
        must(&repo, &["commit", "-q", "-m", "first"]);
        let marker = t.path("marker");
        Rig { t, repo, marker }
    }

    fn script(rig: &Rig, name: &str) -> String {
        let path = rig.t.path(name);
        let body = format!(
            "#!/bin/sh\ntouch '{}'\ncat \"$1\" 2>/dev/null\n",
            rig.marker.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn dirty(rig: &Rig) {
        rig.t.write("repo/c.txt", "one\ntwo\n");
        rig.t.write("repo/b.txt", "fresh\n");
        bump(rig);
    }

    fn bump(rig: &Rig) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(rig.repo.join("a.txt"))
            .unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
    }

    fn the_sandbox_alone_stops_it(rig: &Rig, control: &[&str]) {
        assert!(!rig.marker.exists());
        plain(&rig.repo, control);
        assert!(
            rig.marker.exists(),
            "the trap never fires without the sandbox, so this test would prove nothing"
        );
        std::fs::remove_file(&rig.marker).unwrap();
        bump(rig);
        let mut argv = vec!["git", "-C", rig.repo.to_str().unwrap()];
        argv.extend(control);
        let command = Command::new(&argv)
            .with_env("GIT_CONFIG_GLOBAL", Some("/dev/null"))
            .with_env("GIT_CONFIG_NOSYSTEM", Some("1"));
        let out = sandboxed()
            .run(&command, Duration::from_secs(10))
            .expect("the sandbox starts git");
        assert!(!rig.marker.exists(), "a program started inside the sandbox");
        assert!(
            out.code == 0 || out.code == 128,
            "git itself ran: {}",
            out.stderr
        );
    }

    #[test]
    fn a_clean_filter_cannot_start() {
        let rig = rig("sb-clean");
        rig.t.write("repo/.gitattributes", "* filter=x\n");
        must(
            &rig.repo,
            &["config", "filter.x.clean", &script(&rig, "clean.sh")],
        );
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["diff", "HEAD", "--numstat"]);
    }

    #[test]
    fn a_process_filter_cannot_start() {
        let rig = rig("sb-process");
        rig.t.write("repo/.gitattributes", "* filter=x\n");
        must(
            &rig.repo,
            &["config", "filter.x.process", &script(&rig, "process.sh")],
        );
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["diff", "HEAD", "--numstat"]);
    }

    #[test]
    fn a_shell_command_filter_cannot_start() {
        let rig = rig("sb-shell");
        rig.t.write("repo/.gitattributes", "* filter=x\n");
        let touch = format!("touch '{}'; cat", rig.marker.display());
        must(&rig.repo, &["config", "filter.x.clean", &touch]);
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["diff", "HEAD", "--numstat"]);
    }

    #[test]
    fn a_hook_cannot_start() {
        let rig = rig("sb-hook");
        let hook = rig.repo.join(".git/hooks/post-index-change");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::copy(script(&rig, "hook.sh"), &hook).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["status"]);
    }

    #[test]
    fn an_fsmonitor_program_cannot_start() {
        let rig = rig("sb-fsmonitor");
        must(
            &rig.repo,
            &["config", "core.fsmonitor", &script(&rig, "monitor.sh")],
        );
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["status"]);
    }

    #[test]
    fn a_textconv_program_cannot_start() {
        let rig = rig("sb-textconv");
        rig.t.write("repo/.gitattributes", "*.txt diff=x\n");
        must(
            &rig.repo,
            &["config", "diff.x.textconv", &script(&rig, "textconv.sh")],
        );
        dirty(&rig);
        the_sandbox_alone_stops_it(&rig, &["diff", "HEAD"]);
    }

    #[test]
    fn a_filter_added_between_the_runs_cannot_start() {
        let rig = rig("sb-gap");
        let launcher = sandboxed();
        let cwd = rig.repo.to_string_lossy().into_owned();
        dirty(&rig);
        let first = read_status(&cwd, &launcher).unwrap();
        assert!(first.changes.iter().any(|c| c.path == "c.txt"));
        rig.t.write("repo/.gitattributes", "* filter=evil\n");
        must(
            &rig.repo,
            &["config", "filter.evil.clean", &script(&rig, "evil.sh")],
        );
        let second = read_status(&cwd, &launcher).unwrap();
        let diff = read_diff(&cwd, "c.txt", &launcher).unwrap();
        assert!(!rig.marker.exists(), "a filter that appeared mid-read ran");
        assert!(second.changes.iter().any(|c| c.path == "c.txt"));
        assert!(diff.text.contains("two"));
    }

    #[test]
    fn a_normal_repository_reads_status_counts_and_diffs_under_the_sandbox() {
        let rig = rig("sb-normal");
        dirty(&rig);
        let launcher = sandboxed();
        let cwd = rig.repo.to_string_lossy().into_owned();
        let status = read_status(&cwd, &launcher).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        let c = status.changes.iter().find(|c| c.path == "c.txt").unwrap();
        assert_eq!(
            (c.status.as_str(), c.added, c.removed),
            ("M", Some(1), Some(0))
        );
        assert!(status.changes.iter().any(|c| c.path == "b.txt"));
        let tracked = read_diff(&cwd, "c.txt", &launcher).unwrap();
        assert!(tracked.text.contains("+two"), "{}", tracked.text);
        let fresh = read_diff(&cwd, "b.txt", &launcher).unwrap();
        assert!(fresh.untracked && fresh.text.contains("+fresh"));
        let outside = TempDir::new("sb-norepo");
        let none = read_status(&outside.root().to_string_lossy(), &launcher);
        assert_eq!(none, Err(Failure::NotRepo));
    }

    #[test]
    fn only_git_runs_and_a_failed_sandbox_is_a_refusal_never_a_plain_run() {
        let launcher = sandboxed();
        let got = launcher.run(
            &Command::new(&["/bin/sh", "-c", "echo hi"]),
            Duration::from_secs(5),
        );
        match got {
            Err(LaunchError::Spawn(why)) => assert!(why.starts_with("can't read this repo safely")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_real_system_git_and_the_tools_git_pass_the_ownership_check() {
        let system = Path::new("/usr/bin/git");
        assert!(system.is_file());
        assert!(crate::git::sandbox::trusted(system).is_some());
        let tools = Path::new("/Library/Developer/CommandLineTools/usr/bin/git");
        if tools.is_file() {
            assert!(crate::git::sandbox::trusted(tools).is_some());
        }
    }

    #[test]
    fn a_copy_of_git_in_a_user_owned_folder_is_not_trusted() {
        let t = TempDir::new("sb-copy");
        std::fs::create_dir_all(t.path("bin")).unwrap();
        let copy = t.path("bin/git");
        std::fs::copy("/usr/bin/git", &copy).unwrap();
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(crate::git::sandbox::trusted(&copy).is_none());
    }

    #[test]
    fn a_call_that_comes_before_resolution_ends_waits_a_moment_then_says_still_starting() {
        let slow = Arc::new(crate::control::launcher::FakeLauncher::new());
        slow.set_delay(Duration::from_secs(3));
        let launcher = SandboxedLauncher::waiting(slow, Duration::from_millis(100));
        let started = std::time::Instant::now();
        let got = launcher.run(&Command::new(&["git", "--version"]), Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(2));
        match got {
            Err(LaunchError::Spawn(why)) => {
                assert_eq!(why, "can't read this repo safely: still starting")
            }
            other => panic!("{other:?}"),
        }
    }
}
