use super::read::{read_diff, read_status, Diff};
use super::Status;
use crate::control::RealLauncher;
use crate::sessions::testutil::TempDir;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
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
    assert!(git(dir, args), "git {args:?} failed");
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

fn config(rig: &Rig, key: &str, value: &str) {
    must(&rig.repo, &["config", key, value]);
}

fn dirty(rig: &Rig) {
    rig.t.write("repo/c.txt", "one\ntwo\n");
    rig.t.write("repo/b.txt", "fresh\n");
}

fn bump_mtime(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(30))
        .unwrap();
}

fn launcher() -> RealLauncher {
    RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"))
}

fn reads(rig: &Rig) -> (Status, Diff, Diff) {
    let cwd = rig.repo.to_string_lossy().into_owned();
    let status = read_status(&cwd, &launcher()).unwrap();
    let tracked = read_diff(&cwd, "c.txt", &launcher()).unwrap();
    let fresh = read_diff(&cwd, "b.txt", &launcher()).unwrap();
    (status, tracked, fresh)
}

fn the_reads_run_nothing(rig: &Rig, control: &[&str]) {
    assert!(!rig.marker.exists());
    git(&rig.repo, control);
    assert!(
        rig.marker.exists(),
        "the trap never fires without the guards, so this test would prove nothing"
    );
    std::fs::remove_file(&rig.marker).unwrap();
    let index = rig.repo.join(".git/index");
    let before = std::fs::read(&index).unwrap();
    let (status, tracked, fresh) = reads(rig);
    assert!(!rig.marker.exists(), "a program of the repository ran");
    assert_eq!(
        before,
        std::fs::read(&index).unwrap(),
        "the index was written"
    );
    let c = status.changes.iter().find(|c| c.path == "c.txt").unwrap();
    assert_eq!((c.added, c.removed), (Some(1), Some(0)));
    assert!(status.changes.iter().any(|c| c.path == "b.txt"));
    assert!(tracked.text.contains("+two"), "{}", tracked.text);
    assert!(fresh.untracked && fresh.text.contains("+fresh"));
}

#[test]
fn a_clean_filter_never_runs() {
    let rig = rig("hard-clean");
    rig.t.write("repo/.gitattributes", "* filter=x\n");
    config(&rig, "filter.x.clean", &script(&rig, "clean.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn a_filter_named_with_dots_never_runs() {
    let rig = rig("hard-dots");
    rig.t.write("repo/.gitattributes", "* filter=a.b\n");
    config(&rig, "filter.a.b.clean", &script(&rig, "clean.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn a_filter_set_in_the_info_attributes_never_runs() {
    let rig = rig("hard-info");
    rig.t.write("repo/.git/info/attributes", "* filter=x\n");
    config(&rig, "filter.x.clean", &script(&rig, "clean.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn a_process_filter_never_runs() {
    let rig = rig("hard-process");
    rig.t.write("repo/.gitattributes", "* filter=x\n");
    config(&rig, "filter.x.process", &script(&rig, "process.sh"));
    config(&rig, "filter.x.required", "true");
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn a_post_index_change_hook_never_runs() {
    let rig = rig("hard-hook");
    let hook = rig.repo.join(".git/hooks/post-index-change");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::copy(script(&rig, "hook.sh"), &hook).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    dirty(&rig);
    bump_mtime(&rig.repo.join("a.txt"));
    the_reads_run_nothing(&rig, &["status"]);
}

#[test]
fn an_fsmonitor_program_never_runs() {
    let rig = rig("hard-fsmonitor");
    config(&rig, "core.fsmonitor", &script(&rig, "monitor.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["status"]);
}

#[test]
fn a_textconv_program_never_runs() {
    let rig = rig("hard-textconv");
    rig.t.write("repo/.gitattributes", "*.txt diff=x\n");
    config(&rig, "diff.x.textconv", &script(&rig, "textconv.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD"]);
}

#[test]
fn an_external_diff_program_never_runs() {
    let rig = rig("hard-external");
    config(&rig, "diff.external", &script(&rig, "external.sh"));
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD"]);
}

#[test]
fn a_filter_defined_in_a_user_include_file_never_runs() {
    let rig = rig("hard-include");
    rig.t.write("repo/.gitattributes", "* filter=x\n");
    let include = rig.t.path("extra.cfg");
    std::fs::write(
        &include,
        format!("[filter \"x\"]\n\tclean = {}\n", script(&rig, "clean.sh")),
    )
    .unwrap();
    config(&rig, "include.path", &include.to_string_lossy());
    dirty(&rig);
    the_reads_run_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}
