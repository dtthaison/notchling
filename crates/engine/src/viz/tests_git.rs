//! The Flow tab's git snapshot, through a fake launcher.

use super::engine::GitSnapshot;
use super::tests_urls::folder;

#[test]
fn the_git_snapshot_is_the_branch_and_the_diff_stat_from_the_launcher() {
    use crate::control::{FakeLauncher, LaunchError, LaunchOutput};
    let dir = folder(&[(".git/HEAD", "ref: refs/heads/feat/x\n")], &[]);
    let cwd = dir.root().to_string_lossy().into_owned();
    let launcher = FakeLauncher::new();
    launcher.push_output(Ok(LaunchOutput::ok(" a.swift | 4 ++--\n 1 file changed\n")));
    let snapshot = super::git::read(&cwd, &launcher);
    assert_eq!(snapshot.branch.as_deref(), Some("feat/x"));
    assert_eq!(
        snapshot.diff_stat.as_deref(),
        Some("a.swift | 4 ++--\n 1 file changed")
    );
    let command = &launcher.commands()[0];
    assert!(
        command.argv.contains(&"--no-optional-locks".to_string()),
        "never takes the index lock"
    );
    assert!(command.argv.ends_with(&[
        "diff".into(),
        "--stat".into(),
        "--no-color".into(),
        "--no-ext-diff".into()
    ]));
    assert_eq!(
        command.env.get("GIT_TERMINAL_PROMPT"),
        Some(&Some("0".to_string()))
    );

    // git failing or printing nothing leaves the branch alone.
    launcher.push_output(Ok(LaunchOutput::failed(128, "fatal")));
    assert_eq!(super::git::read(&cwd, &launcher).diff_stat, None);
    launcher.push_output(Ok(LaunchOutput::ok("  \n")));
    assert_eq!(super::git::read(&cwd, &launcher).diff_stat, None);
    launcher.push_output(Err(LaunchError::Timeout(3)));
    let timed_out = super::git::read(&cwd, &launcher);
    assert_eq!(
        (timed_out.branch.as_deref(), timed_out.diff_stat),
        (Some("feat/x"), None)
    );

    // No folder, no process.
    let calls = launcher.calls().len();
    assert_eq!(super::git::read("", &launcher), GitSnapshot::default());
    assert_eq!(
        super::git::read("/definitely/not/here", &launcher),
        GitSnapshot::default()
    );
    assert_eq!(launcher.calls().len(), calls);
}

#[test]
fn a_long_diff_stat_is_cut_on_a_character() {
    use crate::control::{FakeLauncher, LaunchOutput};
    let dir = folder(&[], &[]);
    let launcher = FakeLauncher::new();
    launcher.push_output(Ok(LaunchOutput::ok(&"é".repeat(20_000))));
    let stat = super::git::read(&dir.root().to_string_lossy(), &launcher)
        .diff_stat
        .expect("stat");
    assert_eq!(stat.len(), 16_000);
}
