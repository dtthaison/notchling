use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use super::actions_util::{branch_source, failure, failure_of};
pub use super::actions_util::{destination, is_plain_image_name};
use super::merge::{refused_issue, worktree_url, MergeOutcome, MergePlan};
use super::morning::{MorningEntry, MorningItem};
use super::registry::{Machine, MachineRepo};
use super::runner::{argv, Call, Runner};
use super::shell;
use super::status::ReadyEntry;

pub type MergeFunction =
    Arc<dyn Fn(&ReadyEntry, &Machine, &MachineRepo, &dyn Runner) -> MergeOutcome + Send + Sync>;

pub const COMMENT_PREFIX: &str = "notchling: ";

const NOT_PUSHED_AGAIN: &str = "nothing to push again: no checked merge of this branch is waiting";
const UNDONE: &str = "Merge undone: main is back where it was. Merge again when you are ready.";

#[derive(Default)]
struct State {
    running: BTreeSet<String>,
    last_error: BTreeMap<String, String>,
    outcomes: BTreeMap<String, MergeOutcome>,
    outcome_commits: BTreeMap<String, String>,
    notes: BTreeMap<String, String>,
}

pub struct MorningActions {
    runner: Arc<dyn Runner>,
    machines: Box<dyn Fn() -> Vec<Machine> + Send + Sync>,
    plan: MergePlan,
    merge_fn: Option<MergeFunction>,
    retry_push_fn: Option<MergeFunction>,
    undo_fn: Option<MergeFunction>,
    cache_root: PathBuf,
    state: Mutex<State>,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Holding<'a> {
    actions: &'a MorningActions,
    id: &'a str,
}

impl Drop for Holding<'_> {
    fn drop(&mut self) {
        lock(&self.actions.state).running.remove(self.id);
    }
}

impl MorningActions {
    pub fn new(
        runner: Arc<dyn Runner>,
        machines: impl Fn() -> Vec<Machine> + Send + Sync + 'static,
        cache_root: PathBuf,
    ) -> MorningActions {
        MorningActions {
            runner,
            machines: Box::new(machines),
            plan: MergePlan::from_process(&cache_root.join("merge")),
            merge_fn: None,
            retry_push_fn: None,
            undo_fn: None,
            cache_root,
            state: Mutex::new(State::default()),
        }
    }

    pub fn plan(&self) -> &MergePlan {
        &self.plan
    }

    pub fn with_plan(mut self, plan: MergePlan) -> MorningActions {
        self.plan = plan;
        self
    }

    pub fn with_merge(mut self, f: MergeFunction) -> MorningActions {
        self.merge_fn = Some(f);
        self
    }

    pub fn with_retry_push(mut self, f: MergeFunction) -> MorningActions {
        self.retry_push_fn = Some(f);
        self
    }

    pub fn with_undo_merge(mut self, f: MergeFunction) -> MorningActions {
        self.undo_fn = Some(f);
        self
    }

    pub fn running(&self) -> BTreeSet<String> {
        lock(&self.state).running.clone()
    }

    pub fn is_running(&self, id: &str) -> bool {
        lock(&self.state).running.contains(id)
    }

    pub fn last_error(&self, id: &str) -> Option<String> {
        lock(&self.state).last_error.get(id).cloned()
    }

    pub fn note(&self, id: &str) -> Option<String> {
        lock(&self.state).notes.get(id).cloned()
    }

    pub fn notes(&self) -> BTreeMap<String, String> {
        lock(&self.state).notes.clone()
    }

    pub fn outcomes(&self) -> BTreeMap<String, MergeOutcome> {
        lock(&self.state).outcomes.clone()
    }

    pub fn outcome(&self, item: &MorningItem) -> Option<MergeOutcome> {
        outcome_in(&lock(&self.state), item)
    }

    pub fn prune_outcomes(&self, keeping: &[MorningItem]) {
        let mut s = lock(&self.state);
        let ids: Vec<String> = s.outcomes.keys().cloned().collect();
        for id in ids {
            let kept = keeping
                .iter()
                .find(|i| i.id == id)
                .is_some_and(|i| outcome_in(&s, i).is_some());
            if !kept {
                s.outcomes.remove(&id);
                s.outcome_commits.remove(&id);
            }
        }
        let live: BTreeSet<&str> = keeping.iter().map(|i| i.id.as_str()).collect();
        s.notes.retain(|k, _| live.contains(k.as_str()));
    }

    pub fn merge(&self, item: &MorningItem) -> bool {
        self.merge_like(item, Kind::Merge)
    }

    pub fn retry_push(&self, item: &MorningItem) -> bool {
        self.merge_like(item, Kind::RetryPush)
    }

    pub fn undo_merge(&self, item: &MorningItem) -> bool {
        self.start(item, || {
            let MorningEntry::Ready(entry) = &item.entry else {
                return Some(format!("{} is not a ready branch", item.id));
            };
            let Some((machine, repo)) = self.lookup(item) else {
                return Some(self.refusal(item));
            };
            let undone = match &self.undo_fn {
                Some(f) => f(entry, &machine, &repo, &*self.runner),
                None => self.plan.undo_merge(entry, &machine, &repo, &*self.runner),
            };
            if let Some(error) = undone.error {
                return Some(error);
            }
            let mut s = lock(&self.state);
            s.outcomes.remove(&item.id);
            s.outcome_commits.remove(&item.id);
            s.notes.insert(item.id.clone(), UNDONE.into());
            None
        })
    }

    pub fn send_back(&self, item: &MorningItem, note: &str) -> bool {
        self.comment(
            item,
            &format!("{COMMENT_PREFIX}{note}"),
            &["--add-label", "sd:queued", "--remove-label", "sd:ready"],
        )
    }

    pub fn answer(&self, item: &MorningItem, text: &str) -> bool {
        self.comment(
            item,
            &format!("{COMMENT_PREFIX}{text}"),
            &["--add-label", "sd:queued", "--remove-label", "sd:needs-you"],
        )
    }

    pub fn use_recommended(&self, item: &MorningItem) -> bool {
        self.answer(item, "use recommended")
    }

    pub fn retry(&self, item: &MorningItem) -> bool {
        self.start(item, || {
            if let Some(why) = self.registration_error(item) {
                return Some(why);
            }
            self.edit(
                item,
                &["--add-label", "sd:queued", "--remove-label", "sd:stuck"],
            )
        })
    }

    pub fn close(&self, item: &MorningItem) -> bool {
        self.start(item, || {
            if let Some(why) = self.registration_error(item) {
                return Some(why);
            }
            let issue = item.issue.to_string();
            self.gh(&[
                "issue",
                "close",
                &issue,
                "-R",
                &item.repo,
                "--reason",
                "not planned",
            ])
        })
    }

    pub fn drop_item(&self, item: &MorningItem) -> bool {
        self.close(item)
    }

    pub fn diff(&self, item: &MorningItem) -> String {
        let Some((machine, repo)) = self.lookup(item) else {
            return self.refusal(item);
        };
        let Some((branch, worktree)) = branch_source(item) else {
            return format!("{} has no branch on the machine", item.id);
        };
        let url = match worktree_url(&machine, &worktree) {
            Ok(url) => url,
            Err(why) => return why,
        };
        if let Some(why) = refused_issue(item.issue) {
            return why;
        }
        let _holding = {
            let mut s = lock(&self.state);
            if s.running.contains(&item.id) {
                return format!(
                    "an action is running on {}; look at the diff when it ends",
                    item.id
                );
            }
            s.running.insert(item.id.clone());
            Holding {
                actions: self,
                id: &item.id,
            }
        };
        if let Some(why) = self
            .plan
            .branch_error(&branch, &repo.local_path, &*self.runner)
        {
            return why;
        }
        let reference = format!("refs/machines/{}/{}", machine.name, item.issue);
        let refspec = format!("+refs/heads/{branch}:{reference}");
        let fetch = self.runner.run(
            &self
                .plan
                .git_call(
                    &repo.local_path,
                    &["fetch", "--no-tags", &url, &refspec],
                    120,
                )
                .env("GIT_SSH_COMMAND", shell::GIT_SSH_COMMAND),
        );
        if fetch.code != 0 {
            return failure(&fetch, "git fetch");
        }
        let range = format!("refs/heads/main...{reference}");
        let diff = self
            .runner
            .run(&self.plan.git_call(&repo.local_path, &["diff", &range], 60));
        if diff.code == 0 {
            diff.out
        } else {
            failure(&diff, "git diff")
        }
    }

    pub fn screenshots(&self, item: &MorningItem) -> Vec<PathBuf> {
        let MorningEntry::Ready(entry) = &item.entry else {
            return Vec::new();
        };
        if entry.evidence_dir.is_empty() {
            return Vec::new();
        }
        let Some((machine, _)) = self.lookup(item) else {
            return Vec::new();
        };
        if !shell::is_safe_machine_path(&entry.evidence_dir)
            || refused_issue(item.issue).is_some()
            || machine.name.is_empty()
            || machine.name.contains('/')
            || machine.name == "."
            || machine.name == ".."
        {
            return Vec::new();
        }
        let listing = self.runner.run(&Call::secs(
            shell::ssh_argv(&machine, &["ls", "-1", &entry.evidence_dir]),
            30,
        ));
        if listing.code != 0 {
            return Vec::new();
        }
        let mut names: Vec<&str> = listing
            .out
            .split('\n')
            .filter(|n| is_plain_image_name(n))
            .collect();
        names.sort_unstable();
        if names.is_empty() {
            return Vec::new();
        }
        let dir = self
            .cache_root
            .join(&machine.name)
            .join(item.issue.to_string());
        let _ = std::fs::create_dir_all(&dir);
        let mut copied = Vec::new();
        for name in names {
            let Some(target) = destination(name, &dir) else {
                continue;
            };
            let remote = if entry.evidence_dir.ends_with('/') {
                format!("{}{name}", entry.evidence_dir)
            } else {
                format!("{}/{name}", entry.evidence_dir)
            };
            let call = Call::secs(
                argv(&[
                    "/usr/bin/scp",
                    "-O",
                    "-q",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=8",
                    "--",
                    &format!(
                        "{}@{}:{}",
                        machine.user,
                        machine.host,
                        crate::lanes::quote(&remote)
                    ),
                    &target.to_string_lossy(),
                ]),
                60,
            );
            if self.runner.run(&call).code == 0 {
                copied.push(target);
            }
        }
        copied
    }

    fn merge_like(&self, item: &MorningItem, kind: Kind) -> bool {
        self.start(item, || {
            let MorningEntry::Ready(entry) = &item.entry else {
                return Some(format!("{} is not a ready branch", item.id));
            };
            if kind == Kind::RetryPush && !self.outcome(item).is_some_and(|o| o.can_retry_push()) {
                return Some(NOT_PUSHED_AGAIN.into());
            }
            let Some((machine, repo)) = self.lookup(item) else {
                return Some(self.refusal(item));
            };
            let runner = &*self.runner;
            let outcome = match (kind, &self.merge_fn, &self.retry_push_fn) {
                (Kind::Merge, Some(f), _) => f(entry, &machine, &repo, runner),
                (Kind::Merge, None, _) => self.plan.merge(entry, &machine, &repo, runner),
                (_, _, Some(f)) => f(entry, &machine, &repo, runner),
                (_, _, None) => self.plan.retry_push(entry, &machine, &repo, runner),
            };
            let error = failure_of(&outcome);
            let mut s = lock(&self.state);
            s.outcomes.insert(item.id.clone(), outcome);
            s.outcome_commits
                .insert(item.id.clone(), entry.commit.clone());
            error
        })
    }

    fn comment(&self, item: &MorningItem, body: &str, labels: &[&str]) -> bool {
        self.start(item, || {
            if let Some(why) = self.registration_error(item) {
                return Some(why);
            }
            let issue = item.issue.to_string();
            if let Some(error) =
                self.gh(&["issue", "comment", &issue, "-R", &item.repo, "--body", body])
            {
                return Some(error);
            }
            self.edit(item, labels)
        })
    }

    fn edit(&self, item: &MorningItem, flags: &[&str]) -> Option<String> {
        let issue = item.issue.to_string();
        let mut args = vec!["issue", "edit", &issue, "-R", &item.repo];
        args.extend_from_slice(flags);
        self.gh(&args)
    }

    fn gh(&self, args: &[&str]) -> Option<String> {
        let result = self.runner.run(&Call {
            timeout: std::time::Duration::from_secs(30),
            ..self.plan.gh_call(args)
        });
        (result.code != 0).then(|| {
            let what = format!(
                "gh {}",
                args.iter().take(2).copied().collect::<Vec<_>>().join(" ")
            );
            failure(&result, &what)
        })
    }

    fn start(&self, item: &MorningItem, work: impl FnOnce() -> Option<String>) -> bool {
        {
            let mut s = lock(&self.state);
            if s.running.contains(&item.id) {
                return false;
            }
            s.running.insert(item.id.clone());
            s.last_error.remove(&item.id);
            s.notes.remove(&item.id);
        }
        let _holding = Holding {
            actions: self,
            id: &item.id,
        };
        if let Some(error) = work() {
            lock(&self.state).last_error.insert(item.id.clone(), error);
        }
        true
    }

    fn lookup(&self, item: &MorningItem) -> Option<(Machine, MachineRepo)> {
        let machine = (self.machines)()
            .into_iter()
            .find(|m| m.name == item.machine)?;
        let repo = machine
            .repos()
            .iter()
            .find(|r| r.github == item.repo)?
            .clone();
        Some((machine, repo))
    }

    fn refusal(&self, item: &MorningItem) -> String {
        if (self.machines)().iter().any(|m| m.name == item.machine) {
            format!("repo {} is not registered for {}", item.repo, item.machine)
        } else {
            format!("machine {} is not in the machines list", item.machine)
        }
    }

    fn registration_error(&self, item: &MorningItem) -> Option<String> {
        if self.lookup(item).is_none() {
            return Some(self.refusal(item));
        }
        refused_issue(item.issue)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Merge,
    RetryPush,
}

fn outcome_in(s: &State, item: &MorningItem) -> Option<MergeOutcome> {
    let MorningEntry::Ready(entry) = &item.entry else {
        return None;
    };
    (s.outcome_commits.get(&item.id) == Some(&entry.commit))
        .then(|| s.outcomes.get(&item.id).cloned())
        .flatten()
}
