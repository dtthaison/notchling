use crate::cluster_blobs::Blobs;
use crate::cluster_testkit::Dir;
use crate::cluster_workspace::Workspace;
use crate::cluster_workspace_send::{Peers, Sender};
use notchling_engine::cluster::NodeId;
use notchling_engine::control::plan::PtyScript;
use notchling_engine::control::{Command, LaunchError, LaunchOutput, Launcher, RealLauncher};
use notchling_local_api::ApiError;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub(crate) struct Isolated {
    inner: RealLauncher,
    home: PathBuf,
}

impl Isolated {
    pub(crate) fn new(home: PathBuf) -> Isolated {
        Isolated {
            inner: RealLauncher::with_claude(PathBuf::from("/nonexistent/claude")),
            home,
        }
    }
}

impl Launcher for Isolated {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        let isolated = command
            .clone()
            .with_env("HOME", self.home.to_str())
            .with_env("XDG_CONFIG_HOME", self.home.to_str())
            .with_env("GIT_CONFIG_GLOBAL", Some("/dev/null"))
            .with_env("GIT_CONFIG_NOSYSTEM", Some("1"));
        self.inner.run(&isolated, timeout)
    }

    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

pub(crate) struct Gated {
    inner: Isolated,
    hold: String,
    entered: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

pub(crate) struct Gate {
    entered: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

const GATE_WAIT: Duration = Duration::from_secs(20);

fn wait_flag(flag: &(Mutex<bool>, Condvar)) -> bool {
    let (lock, wake) = flag;
    let mut set = lock.lock().unwrap();
    let deadline = std::time::Instant::now() + GATE_WAIT;
    while !*set {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return false;
        }
        set = wake.wait_timeout(set, left).unwrap().0;
    }
    true
}

fn raise(flag: &(Mutex<bool>, Condvar)) {
    *flag.0.lock().unwrap() = true;
    flag.1.notify_all();
}

impl Gate {
    pub(crate) fn wait_entered(&self) -> bool {
        wait_flag(&self.entered)
    }

    pub(crate) fn open(&self) {
        raise(&self.release);
    }
}

impl Gated {
    pub(crate) fn new(home: PathBuf, hold: &str) -> (Gated, Gate) {
        let entered = Arc::new((Mutex::new(false), Condvar::new()));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let gate = Gate {
            entered: entered.clone(),
            release: release.clone(),
        };
        let gated = Gated {
            inner: Isolated::new(home),
            hold: hold.to_string(),
            entered,
            release,
        };
        (gated, gate)
    }
}

impl Launcher for Gated {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        let clones = command.argv.iter().any(|a| a == "clone");
        if clones && command.argv.iter().any(|a| a.contains(&self.hold)) {
            raise(&self.entered);
            wait_flag(&self.release);
        }
        self.inner.run(command, timeout)
    }

    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError> {
        self.inner.run_pty(script)
    }
}

pub(crate) fn noise(len: usize) -> Vec<u8> {
    let mut seed = 12345u64;
    (0..len)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u8
        })
        .collect()
}

pub(crate) fn git_in(home: &Path, dir: Option<&Path>, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    let out = command
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(crate) struct Direct {
    pub worker: Arc<Workspace>,
    pub log: Mutex<Vec<String>>,
}

impl Peers for Direct {
    fn call(&self, _node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        self.log.lock().unwrap().push(op.to_string());
        self.worker
            .call(op, &args)
            .unwrap_or_else(|| Err(ApiError::bad_request(format!("{op} isn't a workspace op"))))
    }
}

pub(crate) fn node_id(letter: char) -> NodeId {
    NodeId::from_fingerprint(&letter.to_string().repeat(64)).expect("a node id")
}

pub(crate) struct Fixture {
    pub dir: Dir,
    pub home: PathBuf,
    pub origin: PathBuf,
    pub repo: PathBuf,
    pub worker: Arc<Workspace>,
    pub peers: Arc<Direct>,
    pub sender: Arc<Sender>,
    pub worker_cluster: PathBuf,
}

impl Fixture {
    pub(crate) fn new(tag: &str) -> Fixture {
        let dir = Dir::new(tag);
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let origin = dir.path().join("origin.git");
        let repo = dir.path().join("repo");
        git_in(
            &home,
            None,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        git_in(
            &home,
            None,
            &["init", "-q", "-b", "main", repo.to_str().unwrap()],
        );
        git_in(
            &home,
            Some(&repo),
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git_in(&home, Some(&repo), &["add", "a.txt"]);
        git_in(&home, Some(&repo), &["commit", "-q", "-m", "one"]);
        git_in(&home, Some(&repo), &["push", "-q", "-u", "origin", "main"]);
        let worker_cluster = dir.path().join("worker/cluster");
        let sender_cluster = dir.path().join("sender/cluster");
        let launcher: Arc<dyn Launcher> = Arc::new(Isolated::new(home.clone()));
        let worker = Workspace::new(
            worker_cluster.clone(),
            launcher.clone(),
            Blobs::new(worker_cluster.clone()),
        );
        let peers = Arc::new(Direct {
            worker: worker.clone(),
            log: Mutex::new(Vec::new()),
        });
        let sender = Sender::new(sender_cluster, launcher, peers.clone());
        Fixture {
            dir,
            home,
            origin,
            repo,
            worker,
            peers,
            sender,
            worker_cluster,
        }
    }

    pub(crate) fn git(&self, dir: &Path, args: &[&str]) -> String {
        git_in(&self.home, Some(dir), args)
    }

    pub(crate) fn commit(&self, file: &str, content: &str, message: &str) -> String {
        std::fs::write(self.repo.join(file), content).unwrap();
        self.git(&self.repo, &["add", file]);
        self.git(&self.repo, &["commit", "-q", "-m", message]);
        self.git(&self.repo, &["rev-parse", "HEAD"])
    }

    pub(crate) fn second_repo(&self, name: &str) -> (PathBuf, PathBuf) {
        let origin = self.dir.path().join(format!("{name}.git"));
        let repo = self.dir.path().join(name);
        let bare = [
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ];
        git_in(&self.home, None, &bare);
        let work = ["init", "-q", "-b", "main", repo.to_str().unwrap()];
        git_in(&self.home, None, &work);
        self.git(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        std::fs::write(repo.join("b.txt"), "b\n").unwrap();
        self.git(&repo, &["add", "b.txt"]);
        self.git(&repo, &["commit", "-q", "-m", "b"]);
        self.git(&repo, &["push", "-q", "-u", "origin", "main"]);
        (origin, repo)
    }

    pub(crate) fn repo_str(&self) -> &str {
        self.repo.to_str().unwrap()
    }

    pub(crate) fn ops(&self) -> Vec<String> {
        self.peers.log.lock().unwrap().clone()
    }
}
