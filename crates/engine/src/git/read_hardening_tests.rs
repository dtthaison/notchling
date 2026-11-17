use super::branch::read_ref_stat;
use super::branch_file::read_ref_diff;
use super::read::{read_diff, read_status, Diff};
use super::reader::Reader;
use super::Status;
use crate::control::{Launcher, RealLauncher};
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

fn launcher() -> Box<dyn Launcher> {
    let real = RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"));
    #[cfg(target_os = "macos")]
    return Box::new(crate::git::SandboxedLauncher::waiting(
        std::sync::Arc::new(real),
        std::time::Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    Box::new(real)
}

fn reads(rig: &Rig) -> (Status, Diff, Diff) {
    let cwd = rig.repo.to_string_lossy().into_owned();
    let status = read_status(&cwd, &*launcher()).unwrap();
    let tracked = read_diff(&cwd, "c.txt", &*launcher()).unwrap();
    let fresh = read_diff(&cwd, "b.txt", &*launcher()).unwrap();
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

#[test]
fn a_textconv_program_never_runs_for_a_ref() {
    let rig = rig("hard-ref-textconv");
    rig.t.write("repo/.gitattributes", "*.txt diff=x\n");
    must(&rig.repo, &["add", "."]);
    must(&rig.repo, &["commit", "-q", "-m", "attributes"]);
    must(&rig.repo, &["checkout", "-q", "-b", "feat"]);
    rig.t.write("repo/c.txt", "one\ntwo\n");
    must(&rig.repo, &["add", "."]);
    must(&rig.repo, &["commit", "-q", "-m", "work"]);
    config(&rig, "diff.x.textconv", &script(&rig, "textconv.sh"));
    assert!(!rig.marker.exists());
    must(&rig.repo, &["diff", "main", "feat"]);
    assert!(
        rig.marker.exists(),
        "the trap never fires without the guards, so this test would prove nothing"
    );
    std::fs::remove_file(&rig.marker).unwrap();
    let cwd = rig.repo.to_string_lossy().into_owned();
    let stat = read_ref_stat(&cwd, Some("main"), "refs/heads/feat", &*launcher()).unwrap();
    assert_eq!((stat.file_count, stat.added), (1, 1));
    let diff = read_ref_diff(&cwd, "main", "refs/heads/feat", "c.txt", None, &*launcher()).unwrap();
    assert!(diff.text.contains("+two"), "{}", diff.text);
    assert!(!rig.marker.exists(), "a program of the repository ran");
}

#[test]
fn changed_at_of_a_real_status_is_the_newest_changed_file() {
    let rig = rig("hard-changed-at");
    dirty(&rig);
    bump_mtime(&rig.repo.join("b.txt"));
    let cwd = rig.repo.to_string_lossy().into_owned();
    let status = read_status(&cwd, &*launcher()).unwrap();
    let want = std::fs::symlink_metadata(rig.repo.join("b.txt"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert_eq!(status.changed_at, Some(want));
}

fn commit_on_feat(rig: &Rig) {
    rig.t
        .write("repo/.gitattributes", "*.txt diff=x filter=x\n");
    must(&rig.repo, &["add", "."]);
    must(&rig.repo, &["commit", "-q", "-m", "attributes"]);
    must(&rig.repo, &["checkout", "-q", "-b", "feat"]);
    rig.t.write("repo/c.txt", "one\ntwo\n");
    must(&rig.repo, &["add", "."]);
    must(&rig.repo, &["commit", "-q", "-m", "work"]);
    must(
        &rig.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &rig.t.path("lane").to_string_lossy(),
            "main",
        ],
    );
}

fn head_of(rig: &Rig, name: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(&rig.repo)
        .args(["rev-parse", name])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn the_reader_runs_nothing(rig: &Rig, control: &[&str]) {
    assert!(!rig.marker.exists());
    git(&rig.repo, control);
    assert!(
        rig.marker.exists(),
        "the trap never fires without the guards, so this test would prove nothing"
    );
    std::fs::remove_file(&rig.marker).unwrap();
    let cwd = rig.repo.to_string_lossy().into_owned();
    let l = launcher();
    let reader = Reader::open(&cwd, &*l).unwrap();
    let feat = head_of(rig, "feat");
    let main = head_of(rig, "main");
    assert_eq!(reader.worktrees().unwrap().0.len(), 2);
    assert!(reader.refs().unwrap().0.len() >= 2);
    let all = reader.ahead_behind_all("main").unwrap().unwrap();
    assert_eq!(all["refs/heads/feat"].ahead, 1);
    assert_eq!(reader.left_right("main", &feat).unwrap().unwrap().ahead, 1);
    let lines = reader.lines("main", &feat).unwrap().unwrap();
    assert_eq!((lines.added, lines.files), (1, 1));
    assert_eq!(reader.related(&main, &feat), Ok(true));
    assert_eq!(reader.commit_facts(&feat).unwrap().1, "work");
    assert!(!rig.marker.exists(), "a program of the repository ran");
}

#[test]
fn the_reader_never_runs_a_clean_filter() {
    let rig = rig("hard-reader-clean");
    commit_on_feat(&rig);
    config(&rig, "filter.x.clean", &script(&rig, "clean.sh"));
    rig.t.write("repo/c.txt", "one\ntwo\nthree\n");
    the_reader_runs_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn the_reader_never_runs_a_process_filter() {
    let rig = rig("hard-reader-process");
    commit_on_feat(&rig);
    config(&rig, "filter.x.process", &script(&rig, "process.sh"));
    config(&rig, "filter.x.required", "true");
    rig.t.write("repo/c.txt", "one\ntwo\nthree\n");
    the_reader_runs_nothing(&rig, &["diff", "HEAD", "--numstat"]);
}

#[test]
fn the_reader_never_runs_a_post_index_change_hook() {
    let rig = rig("hard-reader-hook");
    commit_on_feat(&rig);
    let hook = rig.repo.join(".git/hooks/post-index-change");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::copy(script(&rig, "hook.sh"), &hook).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    rig.t.write("repo/c.txt", "one\ntwo\nthree\n");
    bump_mtime(&rig.repo.join("a.txt"));
    the_reader_runs_nothing(&rig, &["status"]);
}

#[test]
fn the_reader_never_runs_an_fsmonitor_program() {
    let rig = rig("hard-reader-fsmonitor");
    commit_on_feat(&rig);
    config(&rig, "core.fsmonitor", &script(&rig, "monitor.sh"));
    rig.t.write("repo/c.txt", "one\ntwo\nthree\n");
    the_reader_runs_nothing(&rig, &["status"]);
}

#[test]
fn the_reader_never_runs_a_textconv_program() {
    let rig = rig("hard-reader-textconv");
    commit_on_feat(&rig);
    config(&rig, "diff.x.textconv", &script(&rig, "textconv.sh"));
    the_reader_runs_nothing(&rig, &["diff", "main", "feat"]);
}

#[test]
fn the_reader_never_runs_an_external_diff_program() {
    let rig = rig("hard-reader-external");
    commit_on_feat(&rig);
    config(&rig, "diff.external", &script(&rig, "external.sh"));
    the_reader_runs_nothing(&rig, &["diff", "main", "feat"]);
}
