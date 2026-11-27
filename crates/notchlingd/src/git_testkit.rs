use crate::git_ops::{Folders, GitOps};
use crate::limits_ops_tests::TempDir;
use notchling_engine::control::{
    Command, LaunchError, LaunchOutput, Launcher, PtyScript, RealLauncher,
};
#[cfg(target_os = "macos")]
use notchling_engine::git::SandboxedLauncher;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Debug)]
pub struct Seen {
    pub at: Instant,
    pub cwd: String,
    pub sub: String,
    pub argv: Vec<String>,
}

pub struct Gate {
    release: AtomicBool,
    expired: AtomicBool,
    cap: Duration,
}

impl Gate {
    pub fn open(&self) {
        self.release.store(true, Ordering::SeqCst);
    }

    pub fn expired(&self) -> bool {
        self.expired.load(Ordering::SeqCst)
    }

    fn wait(&self) {
        let end = Instant::now() + self.cap;
        while !self.release.load(Ordering::SeqCst) {
            if Instant::now() >= end {
                self.expired.store(true, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

pub struct Recorded {
    inner: Arc<dyn Launcher>,
    seen: Mutex<Vec<Seen>>,
    delay: Mutex<Option<(String, Duration)>>,
    gate: Mutex<Option<(String, Arc<Gate>)>>,
    old_git: AtomicBool,
    hook: Mutex<Option<(String, Hook)>>,
    failing: Mutex<Option<(String, String)>>,
}

type Hook = Box<dyn FnOnce() + Send>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn real_launcher() -> Arc<dyn Launcher> {
    let real = RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"));
    #[cfg(target_os = "macos")]
    return Arc::new(SandboxedLauncher::waiting(
        Arc::new(real),
        Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    Arc::new(real)
}

impl Recorded {
    pub fn new() -> Arc<Recorded> {
        Arc::new(Recorded {
            inner: real_launcher(),
            seen: Mutex::new(Vec::new()),
            delay: Mutex::new(None),
            gate: Mutex::new(None),
            old_git: AtomicBool::new(false),
            hook: Mutex::new(None),
            failing: Mutex::new(None),
        })
    }

    pub fn delay(&self, sub: &str, by: Option<Duration>) {
        *lock(&self.delay) = by.map(|by| (sub.to_owned(), by));
    }

    pub fn gate(&self, sub: &str, cap: Duration) -> Arc<Gate> {
        let gate = Arc::new(Gate {
            release: AtomicBool::new(false),
            expired: AtomicBool::new(false),
            cap,
        });
        *lock(&self.gate) = Some((sub.to_owned(), Arc::clone(&gate)));
        gate
    }

    pub fn before_once(&self, arg: &str, hook: impl FnOnce() + Send + 'static) {
        *lock(&self.hook) = Some((arg.to_owned(), Box::new(hook)));
    }

    pub fn fail(&self, sub: &str, arg: Option<&str>) {
        *lock(&self.failing) = arg.map(|arg| (sub.to_owned(), arg.to_owned()));
    }

    pub fn old_git(&self, on: bool) {
        self.old_git.store(on, Ordering::SeqCst);
    }

    pub fn seen(&self) -> Vec<Seen> {
        lock(&self.seen).clone()
    }

    pub fn clear(&self) {
        lock(&self.seen).clear();
    }

    pub fn count(&self, sub: &str) -> usize {
        self.seen().iter().filter(|s| s.sub == sub).count()
    }

    pub fn count_at(&self, sub: &str, suffix: &str) -> usize {
        self.seen()
            .iter()
            .filter(|s| s.sub == sub && s.cwd.ends_with(suffix))
            .count()
    }

    pub fn ran(&self, sub: &str, arg: &str) -> bool {
        self.seen()
            .iter()
            .any(|s| s.sub == sub && s.argv.iter().any(|a| a.contains(arg)))
    }
}

fn parts(command: &Command) -> (String, String) {
    let at = command.argv.iter().position(|a| a == "-C");
    match at {
        Some(at) => (
            command.argv.get(at + 1).cloned().unwrap_or_default(),
            command.argv.get(at + 2).cloned().unwrap_or_default(),
        ),
        None => (String::new(), String::new()),
    }
}

impl Launcher for Recorded {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        let (cwd, sub) = parts(command);
        lock(&self.seen).push(Seen {
            at: Instant::now(),
            cwd,
            sub: sub.clone(),
            argv: command.argv.clone(),
        });
        let wait = lock(&self.delay)
            .as_ref()
            .filter(|(name, _)| *name == sub)
            .map(|(_, by)| *by);
        if let Some(by) = wait {
            std::thread::sleep(by);
        }
        let gate = lock(&self.gate)
            .as_ref()
            .filter(|(name, _)| *name == sub)
            .map(|(_, gate)| Arc::clone(gate));
        if let Some(gate) = gate {
            gate.wait();
        }
        let fire = {
            let mut held = lock(&self.hook);
            match held.take() {
                Some((arg, hook)) if command.argv.iter().any(|a| a.contains(&arg)) => Some(hook),
                other => {
                    *held = other;
                    None
                }
            }
        };
        if let Some(hook) = fire {
            hook();
        }
        let broken = lock(&self.failing)
            .as_ref()
            .is_some_and(|(name, arg)| *name == sub && command.argv.iter().any(|a| a == arg));
        if broken {
            return Ok(LaunchOutput {
                code: 2,
                output: String::new(),
                stderr: "fatal: scripted failure\n".to_owned(),
            });
        }
        let asks_ahead_behind = command.argv.iter().any(|a| a.contains("ahead-behind"));
        if self.old_git.load(Ordering::SeqCst) && sub == "for-each-ref" && asks_ahead_behind {
            return Ok(LaunchOutput {
                code: 128,
                output: String::new(),
                stderr: "fatal: unknown field name: ahead-behind:abc\n".to_owned(),
            });
        }
        self.inner.run(command, timeout)
    }

    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError> {
        self.inner.run_pty(script)
    }
}

pub fn git_run(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

pub fn git(dir: &Path, args: &[&str]) {
    let out = git_run(dir, args);
    assert!(out.status.success(), "git {args:?} failed");
}

pub fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = git_run(dir, args);
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn lines(prefix: &str, n: usize) -> String {
    (0..n).map(|i| format!("{prefix}{i}\n")).collect()
}

pub struct RepoRig {
    pub t: TempDir,
    pub repo: PathBuf,
    pub lane_a: PathBuf,
    pub launcher: Arc<Recorded>,
    pub ops: GitOps,
}

impl RepoRig {
    pub fn new(tag: &str) -> RepoRig {
        let t = TempDir::new(tag);
        let repo = t.path("repo");
        let lane_a = t.path("lane-a");
        let lane_b = t.path("lane-b");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        t.write("repo/a.txt", &lines("a", 3));
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        git(&repo, &["branch", "merged"]);
        git(&repo, &["checkout", "-q", "-b", "int"]);
        t.write("repo/i1.txt", &lines("p", 2));
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "int one"]);
        t.write("repo/i2.txt", &lines("q", 3));
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "int two"]);
        t.write("repo/a.txt", &format!("{}more\n", lines("a", 3)));
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "int three"]);
        let add = |dir: &Path, extra: &[&str]| {
            let mut args = vec!["worktree", "add", "-q"];
            args.extend(extra);
            git(dir, &args);
        };
        add(&repo, &["-b", "lane-a", lane_a.to_str().unwrap(), "int"]);
        t.write("lane-a/l1.txt", "x\n");
        git(&lane_a, &["add", "."]);
        git(&lane_a, &["commit", "-q", "-m", "lane one"]);
        t.write("lane-a/l2.txt", "y\ny\n");
        git(&lane_a, &["add", "."]);
        git(&lane_a, &["commit", "-q", "-m", "lane two"]);
        t.write("lane-a/untracked.txt", "u\n");
        add(&repo, &["--detach", lane_b.to_str().unwrap(), "int"]);
        let orphan = git_out(&repo, &["commit-tree", EMPTY_TREE, "-m", "alone"]);
        git(&repo, &["branch", "orphan", &orphan]);
        git(
            &repo,
            &[
                "config",
                "remote.origin.url",
                "https://me:secret@github.com/o/r.git",
            ],
        );
        git(&repo, &["update-ref", "refs/remotes/origin/far", "int"]);
        std::fs::write(repo.join(".git/FETCH_HEAD"), "").unwrap();
        let launcher = Recorded::new();
        let known = vec![t.root().to_path_buf()];
        let folders: Folders = Arc::new(move || known.clone());
        let ops = GitOps::new(launcher.clone(), folders);
        RepoRig {
            t,
            repo,
            lane_a,
            launcher,
            ops,
        }
    }

    pub fn cwd(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }

    pub fn lane(&self, name: &str, before: &[&str], after: &[&str]) -> PathBuf {
        let dir = self.t.path(name);
        let folder = dir.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "add", "-q"];
        args.extend(before);
        args.push(&folder);
        args.extend(after);
        git(&self.repo, &args);
        dir
    }
}
