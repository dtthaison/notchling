use super::branch::{read_branch_stat, read_branches};
use super::branch_file::read_branch_diff;
use super::read::Failure;
use crate::control::{Launcher, RealLauncher};
use crate::sessions::testutil::TempDir;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
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
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
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

fn lines(prefix: &str, n: usize) -> String {
    (0..n).map(|i| format!("{prefix}{i}\n")).collect()
}

struct Rig {
    t: TempDir,
    repo: PathBuf,
}

impl Rig {
    fn cwd(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }
}

fn rig(tag: &str) -> Rig {
    let t = TempDir::new(tag);
    let repo = t.path("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    t.write("repo/a.txt", &lines("a", 4));
    t.write("repo/del.txt", &lines("d", 5));
    t.write("repo/ren.txt", &lines("r", 12));
    t.write("repo/keep.txt", &lines("k", 3));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    git(&repo, &["branch", "other"]);
    git(&repo, &["checkout", "-q", "-b", "feature/x"]);
    t.write("repo/new.txt", &lines("n", 3));
    t.write("repo/a.txt", &format!("{}changed\n", lines("a", 3)));
    git(&repo, &["rm", "-q", "del.txt"]);
    git(&repo, &["mv", "ren.txt", "moved.txt"]);
    t.write("repo/moved.txt", &format!("{}more\n", lines("r", 12)));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "work"]);
    Rig { t, repo }
}

fn dirty(rig: &Rig) {
    rig.t.write("repo/keep.txt", &lines("k", 5));
    rig.t.write("repo/fresh.txt", "x\ny\n");
}

#[test]
fn a_branch_ahead_of_main_lists_added_deleted_renamed_and_changed_files() {
    let rig = rig("branch-real-ahead");
    let stat = read_branch_stat(&rig.cwd(), None, &*launcher()).unwrap();
    assert_eq!(stat.branch.as_deref(), Some("feature/x"));
    assert_eq!(stat.base.as_deref(), Some("main"));
    assert!(!stat.detached && !stat.truncated);
    type Row<'a> = (
        &'a str,
        &'a str,
        Option<&'a str>,
        Option<u64>,
        Option<u64>,
        bool,
    );
    let got: Vec<Row> = stat
        .files
        .iter()
        .map(|f| {
            (
                f.path.as_str(),
                f.status.as_str(),
                f.from.as_deref(),
                f.added,
                f.removed,
                f.uncommitted,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("a.txt", "M", None, Some(1), Some(1), false),
            ("del.txt", "D", None, Some(0), Some(5), false),
            ("moved.txt", "R", Some("ren.txt"), Some(1), Some(0), false),
            ("new.txt", "A", None, Some(3), Some(0), false),
        ]
    );
    assert_eq!((stat.added, stat.removed, stat.file_count), (5, 6, 4));
    assert_eq!(stat.uncommitted_files, 0);
    assert_eq!(stat.merge_base.as_ref().map(String::len), Some(7));
}

#[test]
fn uncommitted_work_is_included_and_marked() {
    let rig = rig("branch-real-dirty");
    dirty(&rig);
    let stat = read_branch_stat(&rig.cwd(), Some("main"), &*launcher()).unwrap();
    let find = |name: &str| stat.files.iter().find(|f| f.path == name).unwrap();
    let keep = find("keep.txt");
    assert_eq!(
        (keep.status.as_str(), keep.added, keep.removed),
        ("M", Some(2), Some(0))
    );
    assert!(keep.uncommitted);
    let fresh = find("fresh.txt");
    assert_eq!(
        (fresh.status.as_str(), fresh.added, fresh.removed),
        ("?", Some(2), Some(0))
    );
    assert!(fresh.uncommitted);
    assert!(!find("new.txt").uncommitted);
    assert_eq!(stat.uncommitted_files, 2);
    assert_eq!(stat.file_count, 6);
    assert_eq!((stat.added, stat.removed), (9, 6));
}

#[test]
fn a_staged_change_to_a_committed_file_counts_once_against_the_base() {
    let rig = rig("branch-real-staged");
    rig.t.write("repo/new.txt", &lines("n", 4));
    git(&rig.repo, &["add", "new.txt"]);
    let stat = read_branch_stat(&rig.cwd(), None, &*launcher()).unwrap();
    let new = stat.files.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(
        (new.status.as_str(), new.added, new.uncommitted),
        ("A", Some(4), true)
    );
}

#[test]
fn the_default_base_follows_origin_head_then_main() {
    let rig = rig("branch-real-default");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/other", "other"],
    );
    git(
        &rig.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/other",
        ],
    );
    let stat = read_branch_stat(&rig.cwd(), None, &*launcher()).unwrap();
    assert_eq!(stat.base.as_deref(), Some("origin/other"));
    let menu = read_branches(&rig.cwd(), &*launcher()).unwrap();
    assert_eq!(menu.remote_default.as_deref(), Some("origin/other"));
    assert_eq!(menu.default.as_deref(), Some("origin/other"));
}

#[test]
fn the_base_menu_lists_the_local_branches() {
    let rig = rig("branch-real-menu");
    let menu = read_branches(&rig.cwd(), &*launcher()).unwrap();
    assert_eq!(menu.current.as_deref(), Some("feature/x"));
    let mut local = menu.local.clone();
    local.sort();
    assert_eq!(local, ["feature/x", "main", "other"]);
    assert_eq!(
        (menu.remote_default, menu.default.as_deref()),
        (None, Some("main"))
    );
    assert!(!menu.truncated);
}

#[test]
fn a_missing_base_fails_and_a_flag_shaped_one_is_refused() {
    let rig = rig("branch-real-missing");
    assert_eq!(
        read_branch_stat(&rig.cwd(), Some("nope"), &*launcher()),
        Err(Failure::Failed("no such branch".to_owned()))
    );
    assert!(matches!(
        read_branch_stat(&rig.cwd(), Some("--output=/tmp/x"), &*launcher()),
        Err(Failure::Refused(_))
    ));
    assert!(matches!(
        read_branch_diff(&rig.cwd(), "nope", "a.txt", None, &*launcher()),
        Err(Failure::Failed(_))
    ));
}

#[test]
fn a_repo_with_one_branch_compares_only_the_uncommitted_work() {
    let t = TempDir::new("branch-real-main");
    let repo = t.path("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    t.write("repo/a.txt", "one\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    t.write("repo/a.txt", "one\ntwo\n");
    let stat = read_branch_stat(&repo.to_string_lossy(), None, &*launcher()).unwrap();
    assert_eq!(stat.base.as_deref(), Some("main"));
    assert_eq!(stat.files.len(), 1);
    assert!(stat.files[0].uncommitted);
}

#[test]
fn a_repo_without_commits_or_a_base_answers_with_no_base() {
    let t = TempDir::new("branch-real-empty");
    let repo = t.path("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "trunk"]);
    t.write("repo/a.txt", "one\n");
    let stat = read_branch_stat(&repo.to_string_lossy(), None, &*launcher()).unwrap();
    assert_eq!((stat.base, stat.files.len()), (None, 0));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let stat = read_branch_stat(&repo.to_string_lossy(), None, &*launcher()).unwrap();
    assert_eq!(stat.base, None);
    assert_eq!(stat.branch.as_deref(), Some("trunk"));
}

#[test]
fn a_folder_that_is_not_a_repository_is_told_apart() {
    let t = TempDir::new("branch-real-norepo");
    t.write("plain/a.txt", "x");
    let cwd = t.path("plain").to_string_lossy().into_owned();
    assert_eq!(
        read_branch_stat(&cwd, None, &*launcher()),
        Err(Failure::NotRepo)
    );
}

#[test]
fn a_file_patch_covers_each_kind_of_change() {
    let rig = rig("branch-real-patch");
    dirty(&rig);
    let cwd = rig.cwd();
    let patch =
        |path: &str, from: Option<&str>| read_branch_diff(&cwd, "main", path, from, &*launcher());
    let changed = patch("a.txt", None).unwrap();
    assert!(changed.text.contains("+changed") && changed.text.contains("-a3"));
    assert!(!changed.untracked && !changed.truncated);
    let deleted = patch("del.txt", None).unwrap();
    assert!(deleted.text.contains("deleted file mode") && deleted.text.contains("-d4"));
    let added = patch("new.txt", None).unwrap();
    assert!(added.text.contains("new file mode") && added.text.contains("+n2"));
    let moved = patch("moved.txt", Some("ren.txt")).unwrap();
    assert!(moved.text.contains("rename from ren.txt") && moved.text.contains("+more"));
    assert!(!moved.text.contains("+r3"));
    let uncommitted = patch("keep.txt", None).unwrap();
    assert!(uncommitted.text.contains("+k4"));
    let fresh = patch("fresh.txt", None).unwrap();
    assert!(fresh.untracked && fresh.text.contains("+x"));
    assert_eq!(
        patch("keep.txt.nope", None),
        Err(Failure::Refused("not a changed file".to_owned()))
    );
    assert!(matches!(patch("../a.txt", None), Err(Failure::Refused(_))));
}

#[test]
fn a_patch_over_the_cap_is_cut_and_marked() {
    let rig = rig("branch-real-big");
    rig.t.write("repo/big.txt", &lines("line number ", 900));
    git(&rig.repo, &["add", "big.txt"]);
    git(&rig.repo, &["commit", "-q", "-m", "big"]);
    let big = read_branch_diff(&rig.cwd(), "main", "big.txt", None, &*launcher()).unwrap();
    assert!(big.truncated);
    assert!(big.text.lines().count() <= super::read::MAX_DIFF_LINES);
}
