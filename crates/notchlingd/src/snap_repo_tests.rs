use crate::limits_ops_tests::TempDir;
use crate::snap_repo::read_repo;
use notchling_engine::cluster::snapshot::{movable, Movable, RepoFacts};
use notchling_engine::control::{Launcher, RealLauncher};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn plain_git(home: &Path, dir: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs")
}

struct Rig {
    t: TempDir,
    work: PathBuf,
    origin: PathBuf,
    marker: PathBuf,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let t = TempDir::new(tag);
        let work = t.path("work");
        let origin = t.path("origin.git");
        let marker = t.path("marker");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&origin).unwrap();
        Rig {
            t,
            work,
            origin,
            marker,
        }
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = plain_git(self.t.root(), dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn w(&self, args: &[&str]) -> String {
        self.git(&self.work, args)
    }

    fn pushed() -> Rig {
        let rig = Rig::new("snaprepo");
        rig.git(&rig.origin, &["init", "-q", "--bare", "-b", "main"]);
        rig.w(&["init", "-q", "-b", "main"]);
        rig.t.write("work/a.txt", "one\n");
        rig.t.write("work/c.txt", "one\n");
        rig.w(&["add", "."]);
        rig.w(&["commit", "-q", "-m", "first"]);
        rig.w(&["remote", "add", "origin", &rig.origin.to_string_lossy()]);
        rig.w(&["push", "-q", "-u", "origin", "main"]);
        rig
    }

    fn commit_in(&self, dir: &Path, file: &str) {
        std::fs::write(dir.join(file), "more\n").unwrap();
        self.git(dir, &["add", "."]);
        self.git(dir, &["commit", "-q", "-m", file]);
    }

    fn read(&self) -> Option<RepoFacts> {
        read_in(&self.work)
    }

    fn root(&self) -> PathBuf {
        std::fs::canonicalize(&self.work).unwrap()
    }
}

fn launcher() -> Box<dyn Launcher> {
    let real = RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"));
    #[cfg(target_os = "macos")]
    return Box::new(notchling_engine::git::SandboxedLauncher::waiting(
        std::sync::Arc::new(real),
        Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    Box::new(real)
}

fn read_in(folder: &Path) -> Option<RepoFacts> {
    read_repo(folder, &*launcher()).unwrap()
}

fn verdict(rig: &Rig) -> Movable {
    let folder = rig.work.to_string_lossy().into_owned();
    movable(rig.read().as_ref(), &folder)
}

fn no(text: String) -> Movable {
    Movable::No(text)
}

fn bump_mtime(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(30))
        .unwrap();
}

#[test]
fn pushed_clean_tree_is_movable() {
    let rig = Rig::pushed();
    let facts = rig.read().unwrap();
    assert_eq!(facts.root, rig.root());
    assert_eq!(facts.branch.as_deref(), Some("main"));
    assert_eq!(facts.upstream.as_deref(), Some("origin/main"));
    assert_eq!(
        facts.origin,
        Some(rig.origin.to_string_lossy().into_owned())
    );
    assert_eq!(facts.head, rig.w(&["rev-parse", "HEAD"]).trim());
    assert_eq!(
        (facts.unpushed, facts.dirty, facts.untracked),
        (0, false, false)
    );
    assert_eq!(facts.main_repo, None);
    assert_eq!(verdict(&rig), Movable::Yes);
}

#[test]
fn uncommitted_change_is_not_movable() {
    let rig = Rig::pushed();
    rig.t.write("work/a.txt", "one\ntwo\n");
    assert_eq!(
        verdict(&rig),
        no(format!("{} has uncommitted changes", rig.root().display()))
    );
}

#[test]
fn a_staged_change_is_not_movable() {
    let rig = Rig::pushed();
    rig.t.write("work/a.txt", "one\ntwo\n");
    rig.w(&["add", "a.txt"]);
    assert!(rig.read().unwrap().dirty);
}

#[test]
fn untracked_file_is_not_movable() {
    let rig = Rig::pushed();
    rig.t.write("work/loose.txt", "x\n");
    assert_eq!(
        verdict(&rig),
        no(format!("{} has untracked files", rig.root().display()))
    );
}

#[test]
fn ignored_file_doesnt_count() {
    let rig = Rig::pushed();
    rig.t.write("work/.gitignore", "*.log\n");
    rig.w(&["add", ".gitignore"]);
    rig.w(&["commit", "-q", "-m", "ignore"]);
    rig.w(&["push", "-q"]);
    rig.t.write("work/build.log", "noise\n");
    let facts = rig.read().unwrap();
    assert!(!facts.untracked && !facts.dirty);
    assert_eq!(verdict(&rig), Movable::Yes);
}

#[test]
fn ahead_of_upstream_is_not_movable() {
    let rig = Rig::pushed();
    rig.commit_in(&rig.work, "b.txt");
    assert_eq!(rig.read().unwrap().unpushed, 1);
    assert_eq!(
        verdict(&rig),
        no("main has 1 commits that aren't pushed".to_owned())
    );
    rig.commit_in(&rig.work, "d.txt");
    assert_eq!(
        verdict(&rig),
        no("main has 2 commits that aren't pushed".to_owned())
    );
}

#[test]
fn behind_upstream_is_movable() {
    let rig = Rig::pushed();
    let other = rig.t.path("other");
    rig.git(
        rig.t.root(),
        &[
            "clone",
            "-q",
            &rig.origin.to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    rig.commit_in(&other, "theirs.txt");
    rig.git(&other, &["push", "-q"]);
    rig.w(&["fetch", "-q"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.unpushed, 0);
    assert_eq!(verdict(&rig), Movable::Yes);
}

#[test]
fn no_upstream_is_not_movable() {
    let rig = Rig::new("snaprepo-nup");
    rig.w(&["init", "-q", "-b", "main"]);
    rig.commit_in(&rig.work, "a.txt");
    let facts = rig.read().unwrap();
    assert_eq!((facts.upstream.clone(), facts.origin.clone()), (None, None));
    assert_eq!(verdict(&rig), no("main has no pushed upstream".to_owned()));
}

#[test]
fn a_branch_with_an_origin_but_no_upstream_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "-b", "topic"]);
    let facts = rig.read().unwrap();
    assert!(facts.origin.is_some() && facts.upstream.is_none());
    assert_eq!(verdict(&rig), no("topic has no pushed upstream".to_owned()));
}

#[test]
fn an_unborn_branch_is_not_movable() {
    let rig = Rig::new("snaprepo-unborn");
    rig.w(&["init", "-q", "-b", "main"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.head, "");
    assert_eq!(verdict(&rig), no("main has no pushed upstream".to_owned()));
}

#[test]
fn not_a_repo_is_not_movable() {
    let rig = Rig::new("snaprepo-none");
    assert_eq!(rig.read(), None);
    assert_eq!(
        verdict(&rig),
        no(format!("{} isn't a git repository", rig.work.display()))
    );
}

#[test]
fn detached_head_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "--detach"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.branch, None);
    assert_eq!(facts.head, rig.w(&["rev-parse", "HEAD"]).trim());
    assert_eq!(
        verdict(&rig),
        no(format!("{} isn't on a branch", rig.root().display()))
    );
}

#[test]
fn linked_worktree_names_its_main_repo() {
    let rig = Rig::pushed();
    let linked = rig.t.path("linked");
    rig.w(&[
        "worktree",
        "add",
        "-q",
        &linked.to_string_lossy(),
        "-b",
        "other",
    ]);
    let facts = read_in(&linked).unwrap();
    assert_eq!(facts.root, std::fs::canonicalize(&linked).unwrap());
    assert_eq!(facts.main_repo, Some(rig.root()));
    assert_eq!(facts.branch.as_deref(), Some("other"));
    assert_eq!(rig.read().unwrap().main_repo, None);
}

#[test]
fn a_dirty_submodule_makes_the_tree_dirty() {
    let rig = Rig::pushed();
    let sub = rig.t.path("sub");
    std::fs::create_dir_all(&sub).unwrap();
    rig.git(&sub, &["init", "-q", "-b", "main"]);
    rig.commit_in(&sub, "s.txt");
    rig.w(&["submodule", "add", "-q", &sub.to_string_lossy(), "mod"]);
    rig.w(&["commit", "-q", "-m", "add submodule"]);
    rig.w(&["push", "-q"]);
    assert!(!rig.read().unwrap().dirty);
    rig.t.write("work/mod/s.txt", "changed\n");
    assert!(rig.read().unwrap().dirty);
}

type Files = BTreeMap<PathBuf, (u64, SystemTime)>;

fn walk(dir: &Path, base: &Path, into: &mut Files) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            walk(&path, base, into);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            into.insert(rel, (meta.len(), meta.modified().unwrap()));
        }
    }
}

struct Picture {
    files: Files,
    index: Vec<u8>,
    index_mtime: SystemTime,
    refs: BTreeMap<PathBuf, Vec<u8>>,
    packed: Option<Vec<u8>>,
    stash: String,
}

fn picture(rig: &Rig) -> Picture {
    let dot = rig.work.join(".git");
    let mut files = Files::new();
    walk(&dot, &dot, &mut files);
    let mut refs = BTreeMap::new();
    for rel in files.keys().filter(|p| p.starts_with("refs")) {
        refs.insert(rel.clone(), std::fs::read(dot.join(rel)).unwrap());
    }
    Picture {
        files,
        index: std::fs::read(dot.join("index")).unwrap(),
        index_mtime: std::fs::metadata(dot.join("index"))
            .unwrap()
            .modified()
            .unwrap(),
        refs,
        packed: std::fs::read(dot.join("packed-refs")).ok(),
        stash: rig.w(&["stash", "list"]),
    }
}

#[test]
fn reading_state_never_changes_the_repo() {
    let rig = Rig::pushed();
    rig.t.write("work/c.txt", "one\ntwo\n");
    rig.t.write("work/loose.txt", "x\n");
    rig.w(&["stash", "push", "-q", "-m", "keep"]);
    rig.t.write("work/loose.txt", "x\n");
    rig.w(&["pack-refs", "--all"]);
    bump_mtime(&rig.work.join("a.txt"));
    let before = picture(&rig);
    assert!(before.packed.is_some());
    assert!(before.stash.contains("keep"));
    let facts = rig.read().unwrap();
    assert!(facts.untracked);
    let after = picture(&rig);
    assert_eq!(before.index, after.index, "the index bytes changed");
    assert_eq!(
        before.index_mtime, after.index_mtime,
        "the index was touched"
    );
    assert_eq!(before.refs, after.refs);
    assert_eq!(before.packed, after.packed);
    assert_eq!(before.stash, after.stash);
    assert_eq!(before.files, after.files, "a file under .git changed");

    let control = plain_git(rig.t.root(), &rig.work, &["status", "--porcelain"]);
    assert!(control.status.success());
    let touched = picture(&rig);
    assert_ne!(
        before.index, touched.index,
        "a plain status leaves the index alone, so this test would prove nothing"
    );
}

fn trap(rig: &Rig, name: &str) -> String {
    let path = rig.t.path(name);
    let body = format!(
        "#!/bin/sh\ntouch '{}'\ncat \"$1\" 2>/dev/null\n",
        rig.marker.display()
    );
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().into_owned()
}

fn must_not_run(rig: &Rig, control: &[&str]) {
    assert!(!rig.marker.exists());
    let out = plain_git(rig.t.root(), &rig.work, control);
    assert!(out.status.success());
    assert!(
        rig.marker.exists(),
        "the trap never fires without the guards, so this test would prove nothing"
    );
    std::fs::remove_file(&rig.marker).unwrap();
    let plain = RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"));
    for launcher in [&plain as &dyn Launcher, &*launcher()] {
        let facts = read_repo(&rig.work, launcher).unwrap().unwrap();
        assert!(!rig.marker.exists(), "a program of the repository ran");
        assert!(facts.dirty);
    }
}

fn make_stale_and_dirty(rig: &Rig) {
    rig.t.write("work/c.txt", "one\ntwo\n");
    bump_mtime(&rig.work.join("a.txt"));
}

#[test]
fn hooks_and_fsmonitor_never_run() {
    let rig = Rig::pushed();
    rig.w(&["config", "core.fsmonitor", &trap(&rig, "monitor.sh")]);
    make_stale_and_dirty(&rig);
    must_not_run(&rig, &["status"]);

    let rig = Rig::pushed();
    let hook = rig.work.join(".git/hooks/post-index-change");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::copy(trap(&rig, "hook.sh"), &hook).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    make_stale_and_dirty(&rig);
    must_not_run(&rig, &["status"]);

    let rig = Rig::pushed();
    rig.t.write("work/.gitattributes", "* filter=x\n");
    rig.w(&["add", ".gitattributes"]);
    rig.w(&["commit", "-q", "-m", "attributes"]);
    rig.w(&["push", "-q"]);
    rig.w(&["config", "filter.x.clean", &trap(&rig, "clean.sh")]);
    make_stale_and_dirty(&rig);
    must_not_run(&rig, &["diff", "HEAD", "--numstat"]);
}
