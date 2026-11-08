//! Ported from `MachinesMergeGitTests.swift`: `MergePlan`, `MorningActions::diff` and undo against
//! real git in a temp folder: an origin (bare), the Mac's `main` checkout, and the worker's repo.
//! The runner maps the `ssh://` URL to the worker's folder and fakes `gh`; everything else is the
//! real [`ProcessRunner`]. The fake-runner tests pin the order of the calls; these pin what git
//! does with them.
//!
//! Isolation: the folder is made under `std::env::temp_dir()` with a name no other test shares;
//! `git` is `/usr/bin/git`; every process gets `GIT_CONFIG_GLOBAL=/dev/null` and
//! `GIT_CONFIG_NOSYSTEM=1` (no signing, hooks or `followTags` of the developer's own); the only
//! remotes are local paths, so nothing touches the network; and the runner refuses any command
//! that names the real user's `~/Library`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::actions::MorningActions;
use super::fakes_jobs::lock;
use super::merge::{MergeOutcome, MergePlan};
use super::morning::{MorningEntry, MorningItem, MorningKind};
use super::process::ProcessRunner;
use super::registry::{Machine, MachineRepo, SdWorkerSettings};
use super::runner::{Call, CommandResult, Runner, REFUSED};
use super::status::ReadyEntry;

pub(super) const GIT: &str = "/usr/bin/git";
/// Keeps the developer's own git config (signing, hooks, followTags) out of the repos.
const GIT_ENV: [(&str, &str); 7] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_AUTHOR_NAME", "t"),
    ("GIT_AUTHOR_EMAIL", "t@t"),
    ("GIT_COMMITTER_NAME", "t"),
    ("GIT_COMMITTER_EMAIL", "t@t"),
];

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Runs a shell line with the user's own git config out of the way; stdout, trimmed.
pub(super) fn sh(root: &str, line: &str) -> String {
    let mut c = Command::new("/bin/sh");
    c.arg("-c").arg(line).env("HOME", root);
    for (k, v) in GIT_ENV {
        c.env(k, v);
    }
    match c.output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => format!("(could not run sh: {e})"),
    }
}

type AfterCall = Box<dyn Fn(&[String]) + Send + Sync>;

/// The real `ProcessRunner` for git and the shell, with the `ssh://` URL of the worker mapped to
/// its folder, `gh` faked, and the login-shell check run with `/bin/sh`.
pub(super) struct LocalRunner {
    pub(super) machine_path: String,
    pub(super) seen: Mutex<Vec<Vec<String>>>,
    /// Runs after each call, before its result is returned: a way to change the world between two steps.
    pub(super) after_call: Mutex<Option<AfterCall>>,
    pub(super) violations: Mutex<Vec<String>>,
}

impl Runner for LocalRunner {
    fn run(&self, call: &Call) -> CommandResult {
        lock(&self.seen).push(call.argv.clone());
        if let Some(why) = Self::violation(&call.argv, call.cwd.as_deref()) {
            lock(&self.violations).push(why.clone());
            return CommandResult::new(REFUSED, "", &why);
        }
        let mut a: Vec<String> = call
            .argv
            .iter()
            .map(|w| {
                if w.starts_with("ssh://") {
                    self.machine_path.clone()
                } else {
                    w.clone()
                }
            })
            .collect();
        if a[0].rsplit('/').next() == Some("gh") {
            return CommandResult::ok("");
        }
        // the check runs through the user's login shell in the app; the test runs it with sh
        if a.len() == 3 && a[1] == "-lc" {
            a = vec!["/bin/sh".into(), "-c".into(), a[2].clone()];
        }
        let mut c = call.clone();
        c.argv = a.clone();
        c.env.retain(|(k, _)| !GIT_ENV.iter().any(|(g, _)| g == k));
        c.env
            .extend(GIT_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let r = ProcessRunner.run(&c);
        if let Some(f) = lock(&self.after_call).as_ref() {
            f(&a);
        }
        r
    }
}

pub(super) struct Fx {
    /// git prints real paths, so the root is one
    pub(super) root: String,
    pub(super) runner: Arc<LocalRunner>,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn entry(issue: i64, branch: &str, commit: &str) -> ReadyEntry {
    ReadyEntry {
        repo: "acme/app".into(),
        issue,
        branch: branch.into(),
        commit: commit.into(),
        worktree: "/w".into(),
        evidence_dir: String::new(),
        ready_at: None,
    }
}

// Test isolation

#[test]
fn the_runner_refuses_anything_that_names_the_real_library_folder() {
    let fx = Fx::new();
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let never = format!(
        "{home}/Library/Application Support/Notchling/merge/never-made-{}",
        fx.root.len()
    );
    let cases: Vec<(Vec<String>, Option<PathBuf>)> = vec![
        (vec!["/bin/mkdir".into(), "-p".into(), never.clone()], None),
        (
            vec![
                GIT.into(),
                "-C".into(),
                never.clone(),
                "worktree".into(),
                "add".into(),
                never.clone(),
                "main".into(),
            ],
            None,
        ),
        (
            vec![
                "/bin/echo".into(),
                "x".into(),
                format!("{home}/Library/Caches/Notchling/machines/m1/7"),
            ],
            None,
        ),
        (
            vec!["/bin/pwd".into()],
            Some(PathBuf::from(format!("{home}/Library"))),
        ),
    ];
    for (argv, cwd) in &cases {
        let mut call = Call::secs(argv.clone(), 5);
        call.cwd = cwd.clone();
        assert_eq!(fx.runner.run(&call).code, REFUSED, "{argv:?}");
    }
    let seen = lock(&fx.runner.violations).clone();
    assert_eq!(seen.len(), cases.len());
    assert!(seen.iter().all(|s| s.contains("test isolation")));
    assert!(!Path::new(&never).exists());

    // a temp folder is fine
    lock(&fx.runner.violations).clear();
    let mut call = Call::secs(vec!["/bin/echo".into(), "hi".into()], 5);
    call.cwd = Some(PathBuf::from(&fx.root));
    assert_eq!(fx.runner.run(&call).out, "hi\n");
    assert!(lock(&fx.runner.violations).is_empty());
}

// Happy path

#[test]
fn happy_path_pushes_only_main_and_leaves_no_tags() {
    let fx = Fx::new();
    let commit = fx.worker_branch(
        "sd/issue-7",
        "f.txt",
        "base\nwork\n",
        Some("v-from-the-machine"),
    );
    let before = fx.origin_main();
    let o = fx.merge(&entry(7, "sd/issue-7", &commit));
    assert_eq!(o.error, None);
    assert!(o.fetched && o.merged && o.checked && o.pushed && o.closed);
    assert_eq!(fx.origin_main(), fx.mac_main());
    assert_ne!(fx.origin_main(), before);
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), commit);
    assert_eq!(
        fx.git(&fx.origin(), "for-each-ref --format='%(refname)'"),
        "refs/heads/main",
        "origin holds main and nothing else"
    );
    assert_eq!(
        fx.git(&fx.mac(), "tag -l"),
        "",
        "no tag came from the machine"
    );
    assert_eq!(fx.git(&fx.mac(), "status --porcelain"), "");
    assert_eq!(
        fx.runner.gh_calls().first().map(|c| c[..3].to_vec()),
        Some(vec!["issue".to_string(), "close".into(), "7".into()])
    );
}

// C1: tags from a machine

/// A tag named `origin/main` left by an agent on an unreviewed commit, brought in by the fetch
/// behind Diff.
#[test]
fn a_tag_from_a_machine_cannot_move_or_push_main() {
    let fx = Fx::new();
    let evil = fx.worker_branch("sd/issue-9", "evil.txt", "EVIL\n", Some("origin/main"));
    let good = fx.worker_branch("sd/issue-10", "good.txt", "good\n", None);
    fx.git(&fx.machine_path(), &format!("tag main {evil}"));

    // the user only looks at #9's diff and rejects it
    let diff = fx.actions().diff(&fx.item(9, &evil, "sd/issue-9"));
    assert!(
        diff.contains("EVIL"),
        "the diff still shows the branch: {diff}"
    );
    assert_eq!(
        fx.git(&fx.mac(), "tag -l"),
        "",
        "the diff fetch planted a tag from the machine"
    );

    // days later #10 is merged
    let o = fx.merge(&entry(10, "sd/issue-10", &good));
    assert_eq!(o.error, None);
    assert!(o.pushed);
    assert_eq!(fx.git(&fx.mac(), "tag -l"), "");
    assert!(
        !fx.origin_files().contains("evil.txt"),
        "the unreviewed commit reached origin: {}",
        fx.origin_subjects()
    );
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), good);
    assert_eq!(
        fx.origin_subjects().split('|').count(),
        3,
        "merge, good, init: {}",
        fx.origin_subjects()
    );
}

/// The reviewer's exact case: only a tag named `origin/main`, left on a commit nobody merged.
#[test]
fn an_origin_main_tag_from_a_machine_does_not_push_an_unreviewed_commit() {
    let fx = Fx::new();
    let evil = fx.worker_branch("sd/issue-9", "evil.txt", "EVIL\n", Some("origin/main"));
    let good = fx.worker_branch("sd/issue-10", "good.txt", "good\n", None);
    let _ = fx.actions().diff(&fx.item(9, &evil, "sd/issue-9"));
    let o = fx.merge(&entry(10, "sd/issue-10", &good));
    assert_eq!(o.error, None);
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), good);
    assert!(
        !fx.origin_files().contains("evil.txt"),
        "an unreviewed commit reached origin: {}",
        fx.origin_subjects()
    );
}

/// Tags that are already in the user's repo (left by an older fetch) must not redirect a short name.
#[test]
fn tags_already_in_the_repo_do_not_redirect_main_or_origin_main() {
    let fx = Fx::new();
    let mac = fx.mac();
    fx.git(&mac, "checkout -q -b scratch");
    fx.write(&mac, "evil.txt", "EVIL\n");
    fx.git(&mac, "add -A");
    fx.git(&mac, "commit -q -m evil");
    let evil = fx.git(&mac, "rev-parse HEAD");
    fx.git(&mac, "checkout -q main");
    fx.git(&mac, &format!("tag origin/main {evil}"));
    fx.git(&mac, &format!("tag main {evil}"));
    fx.git(&mac, "branch -q -D scratch");
    let commit = fx.branch("sd/issue-7");

    let o = fx.merge(&entry(7, "sd/issue-7", &commit));
    assert_eq!(o.error, None);
    assert!(o.pushed);
    assert!(
        !fx.origin_files().contains("evil.txt"),
        "{}",
        fx.origin_subjects()
    );
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), commit);
}

/// A tag on the machine with the branch's own name must not stand in for the branch.
#[test]
fn a_tag_with_the_branchs_name_on_the_machine_is_not_the_branch() {
    let fx = Fx::new();
    let evil = fx.worker_branch("evil-side", "evil.txt", "EVIL\n", None);
    let commit = fx.branch("sd/issue-7");
    fx.git(&fx.machine_path(), &format!("tag sd/issue-7 {evil}"));
    let o = fx.merge(&entry(7, "sd/issue-7", &commit));
    assert_eq!(o.error, None);
    assert_eq!(fx.git(&fx.origin(), "rev-parse refs/heads/main^2"), commit);
    assert!(!fx.origin_files().contains("evil.txt"));
}

impl LocalRunner {
    /// The real home's Library folder, spelled both ways (git prints real paths).
    fn library_folders() -> Vec<String> {
        let Ok(home) = std::env::var("HOME") else {
            return Vec::new();
        };
        let real = std::fs::canonicalize(&home)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| home.clone());
        let mut v = vec![format!("{home}/Library"), format!("{real}/Library")];
        v.dedup();
        v
    }

    fn violation(argv: &[String], cwd: Option<&Path>) -> Option<String> {
        let mut texts: Vec<String> = argv.to_vec();
        texts.extend(cwd.map(|c| c.to_string_lossy().into_owned()));
        for text in &texts {
            if let Some(folder) = Self::library_folders()
                .iter()
                .find(|f| text.contains(f.as_str()))
            {
                return Some(format!(
                    "test isolation: refused {} (cwd {cwd:?}): it names {folder}, the real user's data",
                    argv.join(" ")
                ));
            }
        }
        None
    }

    pub(super) fn calls(&self) -> Vec<Vec<String>> {
        lock(&self.seen).clone()
    }

    pub(super) fn gh_calls(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|c| c[0].rsplit('/').next() == Some("gh"))
            .map(|c| c[1..].to_vec())
            .collect()
    }

    pub(super) fn clear(&self) {
        lock(&self.seen).clear();
    }
}

impl Fx {
    pub(super) fn new() -> Fx {
        let raw = std::env::temp_dir().join(format!(
            "machines-git-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&raw);
        let root = std::fs::canonicalize(&raw)
            .unwrap_or(raw)
            .to_string_lossy()
            .into_owned();
        let fx = Fx {
            runner: Arc::new(LocalRunner {
                machine_path: format!("{root}/machine"),
                seen: Mutex::default(),
                after_call: Mutex::default(),
                violations: Mutex::default(),
            }),
            root,
        };
        let (origin, mac, machine) = (fx.origin(), fx.mac(), fx.machine_path());
        fx.sh(&format!("{GIT} init -q --bare -b main '{origin}'"));
        fx.sh(&format!("{GIT} init -q -b main '{mac}'"));
        fx.config(&mac);
        fx.write(&mac, "f.txt", "base\n");
        fx.git(&mac, "add f.txt");
        fx.git(&mac, "commit -q -m init");
        fx.git(&mac, &format!("remote add origin '{origin}'"));
        fx.git(&mac, "push -q -u origin main");
        fx.sh(&format!(
            "{GIT} clone -q '{origin}' '{machine}' 2>/dev/null"
        ));
        fx.config(&machine);
        fx
    }

    pub(super) fn origin(&self) -> String {
        format!("{}/origin.git", self.root)
    }
    pub(super) fn mac(&self) -> String {
        format!("{}/mac", self.root)
    }
    pub(super) fn machine_path(&self) -> String {
        format!("{}/machine", self.root)
    }

    pub(super) fn sh(&self, line: &str) -> String {
        sh(&self.root, line)
    }

    pub(super) fn git(&self, dir: &str, args: &str) -> String {
        self.sh(&format!("{GIT} -C '{dir}' {args}"))
    }

    pub(super) fn config(&self, dir: &str) {
        self.git(dir, "config user.email t@t");
        self.git(dir, "config user.name t");
        self.git(dir, "config commit.gpgsign false");
    }

    pub(super) fn write(&self, dir: &str, file: &str, text: &str) {
        let _ = std::fs::write(format!("{dir}/{file}"), text);
    }

    /// A branch on the worker with one commit on top of main; returns its sha.
    pub(super) fn worker_branch(
        &self,
        name: &str,
        file: &str,
        text: &str,
        tag: Option<&str>,
    ) -> String {
        let m = self.machine_path();
        self.git(&m, &format!("checkout -q -b '{name}' main"));
        self.write(&m, file, text);
        self.git(&m, "add -A");
        self.git(&m, &format!("commit -q -m 'work on {name}'"));
        if let Some(tag) = tag {
            self.git(&m, &format!("tag '{tag}'"));
        }
        let sha = self.git(&m, "rev-parse HEAD");
        self.git(&m, "checkout -q main");
        sha
    }

    pub(super) fn branch(&self, name: &str) -> String {
        self.worker_branch(name, "f.txt", "base\nwork\n", None)
    }

    pub(super) fn repo(&self, check: &str) -> MachineRepo {
        MachineRepo {
            github: "acme/app".into(),
            local_path: self.mac(),
            check_command: check.into(),
            machine_path: None,
            verify_skill: None,
        }
    }

    pub(super) fn machine(&self, repo: &MachineRepo) -> Machine {
        let mut m = Machine::new("m1", "m1.tail", "agent");
        let mut job = SdWorkerSettings::new("m1");
        job.sd_path = "/sd.py".into();
        job.repos = vec![repo.clone()];
        m.set_sd_worker(Some(job));
        m
    }

    pub(super) fn plan(&self) -> MergePlan {
        MergePlan {
            // the app's own worktree would go under here, never under the real home
            home: self.root.clone(),
            merge_root: format!("{}/data/daemon/machines/merge", self.root),
            environment: Default::default(),
            git: GIT.into(),
            gh: "/usr/bin/gh".into(),
        }
    }

    pub(super) fn merge_with(&self, e: &ReadyEntry, check: &str) -> MergeOutcome {
        let r = self.repo(check);
        self.plan().merge(e, &self.machine(&r), &r, &*self.runner)
    }

    pub(super) fn merge(&self, e: &ReadyEntry) -> MergeOutcome {
        self.merge_with(e, "true")
    }

    pub(super) fn undo(&self, e: &ReadyEntry) -> MergeOutcome {
        let r = self.repo("true");
        self.plan()
            .undo_merge(e, &self.machine(&r), &r, &*self.runner)
    }

    pub(super) fn retry(&self, e: &ReadyEntry) -> MergeOutcome {
        let r = self.repo("true");
        self.plan()
            .retry_push(e, &self.machine(&r), &r, &*self.runner)
    }

    pub(super) fn origin_main(&self) -> String {
        self.git(&self.origin(), "rev-parse refs/heads/main")
    }

    pub(super) fn mac_main(&self) -> String {
        self.git(&self.mac(), "rev-parse refs/heads/main")
    }

    pub(super) fn origin_subjects(&self) -> String {
        self.git(&self.origin(), "log --format=%s refs/heads/main")
            .replace('\n', "|")
    }

    pub(super) fn origin_files(&self) -> String {
        self.git(&self.origin(), "ls-tree -r --name-only refs/heads/main")
            .replace('\n', "|")
    }

    pub(super) fn assert_nothing_reached_origin(&self, before: &str) {
        assert_eq!(self.origin_main(), before, "origin/main moved");
        let calls = self.runner.calls();
        assert!(
            !calls.iter().any(|c| c.contains(&"push".to_string())),
            "something was pushed"
        );
        assert!(self.runner.gh_calls().is_empty(), "gh was called");
    }

    pub(super) fn item(&self, issue: i64, commit: &str, branch: &str) -> MorningItem {
        MorningItem {
            id: format!("m1|acme/app|{issue}"),
            kind: MorningKind::Ready,
            machine: "m1".into(),
            repo: "acme/app".into(),
            issue,
            title: "t".into(),
            entry: MorningEntry::Ready(entry(issue, branch, commit)),
            comment: None,
        }
    }

    pub(super) fn actions(&self) -> MorningActions {
        let r = self.repo("true");
        let m = self.machine(&r);
        MorningActions::new(
            self.runner.clone(),
            move || vec![m.clone()],
            PathBuf::from(format!("{}/cache", self.root)),
        )
        .with_plan(self.plan())
    }
}
