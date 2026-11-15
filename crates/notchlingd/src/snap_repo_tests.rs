use crate::snap_repo::read_repo;
use notchling_engine::cluster::snapshot::{movable, Movable, RepoFacts};
use notchling_engine::control::{
    Command, LaunchError, LaunchOutput, Launcher, PtyScript, RealLauncher,
};
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

pub(super) struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nl-snaprepo-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub(super) fn root(&self) -> &Path {
        &self.0
    }

    pub(super) fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub(super) fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn plain_git(home: &Path, dir: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Process::new("git");
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

pub(super) struct Rig {
    pub(super) t: Scratch,
    pub(super) work: PathBuf,
    pub(super) origin: PathBuf,
    pub(super) marker: PathBuf,
}

impl Rig {
    pub(super) fn new(tag: &str) -> Rig {
        let t = Scratch::new(tag);
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

    pub(super) fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = plain_git(self.t.root(), dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub(super) fn w(&self, args: &[&str]) -> String {
        self.git(&self.work, args)
    }

    pub(super) fn pushed() -> Rig {
        let rig = Rig::new("pushed");
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

    pub(super) fn commit_in(&self, dir: &Path, file: &str) {
        std::fs::write(dir.join(file), "more\n").unwrap();
        self.git(dir, &["add", "."]);
        self.git(dir, &["commit", "-q", "-m", file]);
    }

    pub(super) fn read(&self) -> Option<RepoFacts> {
        read_in(&self.t, &self.work)
    }

    pub(super) fn root(&self) -> PathBuf {
        std::fs::canonicalize(&self.work).unwrap()
    }
}

pub(super) struct Isolated {
    inner: Box<dyn Launcher>,
    home: PathBuf,
}

impl Isolated {
    fn tame(&self, command: &Command) -> Command {
        command
            .clone()
            .with_env("HOME", self.home.to_str())
            .with_env("GIT_CONFIG_GLOBAL", Some("/dev/null"))
            .with_env("GIT_CONFIG_NOSYSTEM", Some("1"))
    }
}

impl Launcher for Isolated {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        self.inner.run(&self.tame(command), timeout)
    }

    fn run_capped(
        &self,
        command: &Command,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<(LaunchOutput, bool), LaunchError> {
        self.inner
            .run_capped(&self.tame(command), timeout, max_bytes)
    }

    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError> {
        self.inner.run_pty(script)
    }
}

pub(super) fn real() -> RealLauncher {
    RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"))
}

pub(super) fn unsandboxed(t: &Scratch) -> Isolated {
    Isolated {
        inner: Box::new(real()),
        home: t.root().to_path_buf(),
    }
}

pub(super) fn launcher(t: &Scratch) -> Isolated {
    #[cfg(target_os = "macos")]
    let inner: Box<dyn Launcher> = Box::new(notchling_engine::git::SandboxedLauncher::waiting(
        std::sync::Arc::new(real()),
        Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    let inner: Box<dyn Launcher> = Box::new(real());
    Isolated {
        inner,
        home: t.root().to_path_buf(),
    }
}

pub(super) fn read_in(t: &Scratch, folder: &Path) -> Option<RepoFacts> {
    read_repo(folder, &launcher(t)).unwrap()
}

fn verdict(rig: &Rig) -> Movable {
    let folder = rig.work.to_string_lossy().into_owned();
    movable(rig.read().as_ref(), &folder)
}

fn no(text: &str) -> Movable {
    Movable::No(text.to_owned())
}

pub(super) fn bump_mtime(path: &Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(secs))
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
        (
            facts.unpushed,
            facts.dirty,
            facts.untracked,
            facts.submodules
        ),
        (0, false, false, false)
    );
    assert_eq!(facts.main_repo, None);
    assert_eq!(verdict(&rig), Movable::Yes);
}

#[test]
fn uncommitted_change_is_not_movable() {
    let rig = Rig::pushed();
    rig.t.write("work/a.txt", "one\ntwo\n");
    let root = rig.root().display().to_string();
    assert_eq!(
        verdict(&rig),
        no(&format!("{root} has uncommitted changes"))
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
fn hidden_edits_are_not_movable() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let rig = Rig::pushed();
        rig.w(&["update-index", flag, "a.txt"]);
        rig.t.write("work/a.txt", "hidden\n");
        let root = rig.root().display().to_string();
        assert_eq!(
            verdict(&rig),
            no(&format!("{root} has uncommitted changes")),
            "{flag}"
        );
    }
}

#[test]
fn untracked_file_is_not_movable() {
    let rig = Rig::pushed();
    rig.t.write("work/loose.txt", "x\n");
    let root = rig.root().display().to_string();
    assert_eq!(verdict(&rig), no(&format!("{root} has untracked files")));
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
    assert_eq!(verdict(&rig), no("main has 1 commits that aren't pushed"));
    rig.commit_in(&rig.work, "d.txt");
    assert_eq!(verdict(&rig), no("main has 2 commits that aren't pushed"));
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
    assert_eq!(rig.read().unwrap().unpushed, 0);
    assert_eq!(verdict(&rig), Movable::Yes);
}

#[test]
fn no_upstream_is_not_movable() {
    let rig = Rig::new("noup");
    rig.w(&["init", "-q", "-b", "main"]);
    rig.commit_in(&rig.work, "a.txt");
    let facts = rig.read().unwrap();
    assert_eq!((facts.upstream.clone(), facts.origin.clone()), (None, None));
    assert_eq!(verdict(&rig), no("main has no pushed upstream"));
}

#[test]
fn a_branch_with_an_origin_but_no_upstream_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "-b", "topic"]);
    let facts = rig.read().unwrap();
    assert!(facts.origin.is_some() && facts.upstream.is_none());
    assert_eq!(verdict(&rig), no("topic has no pushed upstream"));
}

#[test]
fn local_tracking_branch_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "-b", "topic", "--track", "main"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.upstream.as_deref(), Some("main"));
    assert_eq!(verdict(&rig), no("topic has no pushed upstream"));
}

#[test]
fn upstream_on_another_remote_is_not_movable() {
    let rig = Rig::pushed();
    let fork = rig.t.path("fork.git");
    std::fs::create_dir_all(&fork).unwrap();
    rig.git(&fork, &["init", "-q", "--bare", "-b", "main"]);
    rig.w(&["remote", "add", "fork", &fork.to_string_lossy()]);
    rig.w(&["push", "-q", "-u", "fork", "main"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.upstream.as_deref(), Some("fork/main"));
    assert_eq!(facts.unpushed, 0);
    assert_eq!(verdict(&rig), no("main has no pushed upstream"));
}

#[test]
fn an_upstream_deleted_on_origin_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "-b", "topic"]);
    rig.w(&["push", "-q", "-u", "origin", "topic"]);
    rig.w(&["push", "-q", "origin", "--delete", "topic"]);
    rig.w(&["fetch", "-q", "--prune"]);
    assert_eq!(rig.read().unwrap().upstream, None);
    assert_eq!(verdict(&rig), no("topic has no pushed upstream"));
}

#[test]
fn an_unborn_branch_is_not_movable() {
    let rig = Rig::new("unborn");
    rig.w(&["init", "-q", "-b", "main"]);
    assert_eq!(rig.read().unwrap().head, "");
    assert_eq!(verdict(&rig), no("main has no pushed upstream"));
}

#[test]
fn not_a_repo_is_not_movable() {
    let rig = Rig::new("none");
    assert_eq!(rig.read(), None);
    assert_eq!(
        verdict(&rig),
        no(&format!("{} isn't a git repository", rig.work.display()))
    );
}

#[test]
fn detached_head_is_not_movable() {
    let rig = Rig::pushed();
    rig.w(&["checkout", "-q", "--detach"]);
    let facts = rig.read().unwrap();
    assert_eq!(facts.branch, None);
    assert_eq!(facts.head, rig.w(&["rev-parse", "HEAD"]).trim());
    let root = rig.root().display().to_string();
    assert_eq!(verdict(&rig), no(&format!("{root} isn't on a branch")));
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
    let facts = read_in(&rig.t, &linked).unwrap();
    assert_eq!(facts.root, std::fs::canonicalize(&linked).unwrap());
    assert_eq!(facts.main_repo, Some(rig.root()));
    assert_eq!(facts.branch.as_deref(), Some("other"));
    assert_eq!(rig.read().unwrap().main_repo, None);
}

#[test]
fn repo_with_submodules_is_not_movable() {
    let rig = Rig::pushed();
    add_submodule(&rig);
    let facts = rig.read().unwrap();
    assert!(facts.submodules);
    assert!(!facts.dirty);
    let root = rig.root().display().to_string();
    assert_eq!(verdict(&rig), no(&format!("{root} has submodules")));
}

pub(super) fn add_submodule(rig: &Rig) {
    let sub = rig.t.path("sub");
    std::fs::create_dir_all(&sub).unwrap();
    rig.git(&sub, &["init", "-q", "-b", "main"]);
    rig.t.write("sub/.gitattributes", "* filter=x\n");
    rig.t.write("sub/s.txt", "one\n");
    rig.git(&sub, &["add", "."]);
    rig.git(&sub, &["commit", "-q", "-m", "sub"]);
    rig.w(&["submodule", "add", "-q", &sub.to_string_lossy(), "mod"]);
    rig.w(&["commit", "-q", "-m", "add submodule"]);
    rig.w(&["push", "-q"]);
}

#[test]
fn the_test_launcher_ignores_the_global_config() {
    let rig = Rig::new("global");
    rig.t.write(".gitconfig", "[user]\n\tname = leaked\n");
    let ask = Command::new(&["git", "config", "--global", "--get", "user.name"]);
    for launcher in [
        &launcher(&rig.t) as &dyn Launcher,
        &unsandboxed(&rig.t) as &dyn Launcher,
    ] {
        let out = launcher.run(&ask, Duration::from_secs(10)).unwrap();
        assert_eq!(out.code, 1, "{}", out.output);
        assert_eq!(out.output, "");
    }
}
