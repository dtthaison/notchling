use std::collections::BTreeMap;
use std::path::Path;

use super::registry::{Machine, MachineRepo};
use super::runner::{argv, Call, CommandResult, Runner};
use super::shell;
use super::status::ReadyEntry;

pub(super) const FETCH_TIMEOUT: u64 = 300;
pub(super) const GIT_TIMEOUT: u64 = 60;
pub(super) const CHECK_TIMEOUT: u64 = 1800;
pub(super) const LOCAL_MAIN: &str = "refs/heads/main";
pub(super) const ORIGIN_MAIN: &str = "refs/remotes/origin/main";
pub(super) const UNPUSHED: &str = "refs/remotes/origin/main..refs/heads/main";
pub(super) const FETCH_ORIGIN: [&str; 4] = [
    "fetch",
    "--no-tags",
    "origin",
    "+refs/heads/main:refs/remotes/origin/main",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeTarget {
    Existing(String),
    Own(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    pub fetched: bool,
    pub merged: bool,
    pub checked: bool,
    pub pushed: bool,
    pub closed: bool,
    pub commit: Option<String>,
    pub error: Option<String>,
    pub conflict_files: Vec<String>,
}

impl MergeOutcome {
    pub fn failed(why: String) -> MergeOutcome {
        MergeOutcome {
            error: Some(why),
            ..MergeOutcome::default()
        }
    }

    pub fn can_retry_push(&self) -> bool {
        self.merged && self.checked && !self.pushed
    }

    pub fn can_undo(&self) -> bool {
        self.merged && !self.pushed
    }
}

#[derive(Clone, Debug)]
pub struct MergePlan {
    pub home: String,
    pub merge_root: String,
    pub environment: BTreeMap<String, String>,
    pub git: String,
    pub gh: String,
}

impl MergePlan {
    pub fn from_process(merge_root: &Path) -> MergePlan {
        MergePlan {
            home: std::env::var("HOME").unwrap_or_else(|_| "/var/empty".into()),
            merge_root: merge_root.to_string_lossy().into_owned(),
            environment: std::env::vars().collect(),
            git: shell::tool_path("git"),
            gh: shell::tool_path("gh"),
        }
    }

    pub fn target(&self, repo: &MachineRepo, worktree_list: &str) -> MergeTarget {
        for block in worktree_list.split("\n\n") {
            let mut path: Option<&str> = None;
            let mut on_main = false;
            let mut skip = false;
            for line in block.split(['\n', '\r']).filter(|l| !l.is_empty()) {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(p);
                } else if line == "branch refs/heads/main" {
                    on_main = true;
                } else if line == "bare" || line == "prunable" || line.starts_with("prunable ") {
                    skip = true;
                }
            }
            if let (true, false, Some(p)) = (on_main, skip, path) {
                if !p.is_empty() {
                    return MergeTarget::Existing(p.into());
                }
            }
        }
        let slug = repo.github.replace('/', "-");
        MergeTarget::Own(
            Path::new(&self.merge_root)
                .join(slug)
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub(super) fn git_call(&self, path: &str, args: &[&str], secs: u64) -> Call {
        let mut v = vec![self.git.clone(), "-C".into(), path.into()];
        v.extend(args.iter().map(|a| a.to_string()));
        Call::secs(v, secs)
    }

    pub(super) fn gh_call(&self, args: &[&str]) -> Call {
        let mut v = vec![self.gh.clone()];
        v.extend(args.iter().map(|a| a.to_string()));
        Call::secs(v, GIT_TIMEOUT)
    }

    pub(super) fn resolve_target(
        &self,
        repo: &MachineRepo,
        runner: &dyn Runner,
    ) -> Result<String, String> {
        let list = runner.run(&self.git_call(
            &repo.local_path,
            &["worktree", "list", "--porcelain"],
            GIT_TIMEOUT,
        ));
        if list.code != 0 {
            return Err(format!(
                "git worktree list failed in {}: {}",
                repo.local_path,
                tail(&list)
            ));
        }
        match self.target(repo, &list.out) {
            MergeTarget::Existing(path) => Ok(path),
            MergeTarget::Own(path) => {
                if !Path::new(&path).is_absolute() {
                    return Err(format!("the merge folder {path} is not absolute"));
                }
                let parent = Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mkdir = runner.run(&Call::secs(
                    argv(&["/bin/mkdir", "-p", &parent]),
                    GIT_TIMEOUT,
                ));
                if mkdir.code != 0 {
                    return Err(format!("cannot make {parent}: {}", tail(&mkdir)));
                }
                let add = runner.run(&self.git_call(
                    &repo.local_path,
                    &["worktree", "add", &path, "main"],
                    GIT_TIMEOUT,
                ));
                if add.code != 0 {
                    return Err(format!(
                        "cannot add the main worktree at {path}: {}",
                        tail(&add)
                    ));
                }
                Ok(path)
            }
        }
    }

    pub fn merge(
        &self,
        e: &ReadyEntry,
        machine: &Machine,
        repo: &MachineRepo,
        runner: &dyn Runner,
    ) -> MergeOutcome {
        let mut o = MergeOutcome::default();
        if let Some(why) = MergePlan::refused_branch(&e.branch) {
            o.error = Some(why);
            return o;
        }
        if let Some(why) = refused_issue(e.issue) {
            o.error = Some(why);
            return o;
        }
        let machine_url = match worktree_url(machine, &e.worktree) {
            Ok(url) => url,
            Err(why) => {
                o.error = Some(why);
                return o;
            }
        };
        let path = match self.resolve_target(repo, runner) {
            Ok(p) => p,
            Err(why) => {
                o.error = Some(why);
                return o;
            }
        };
        let git = |args: &[&str]| runner.run(&self.git_call(&path, args, GIT_TIMEOUT));
        let fetch = |args: &[&str]| runner.run(&self.git_call(&path, args, FETCH_TIMEOUT));
        let issue = e.issue.to_string();
        let reference = format!("refs/machines/{}/{}", machine.name, e.issue);

        if let Some(why) = self.git_refuses_branch(&e.branch, &path, runner) {
            o.error = Some(why);
            return o;
        }

        let status = git(&["status", "--porcelain"]);
        if status.code != 0 {
            o.error = Some(format!("git status failed in {path}: {}", tail(&status)));
            return o;
        }
        if !trimmed(&status).is_empty() {
            o.error = Some(format!("main has local changes in {path}"));
            return o;
        }

        let fetch_origin = fetch(&FETCH_ORIGIN);
        if fetch_origin.code != 0 {
            o.error = Some(format!("fetch origin main failed: {}", tail(&fetch_origin)));
            return o;
        }
        let catch_up = git(&["merge", "--ff-only", ORIGIN_MAIN]);
        if catch_up.code != 0 {
            o.error = Some(format!(
                "main cannot fast-forward to origin/main: {}",
                tail(&catch_up)
            ));
            return o;
        }
        let ahead = git(&["rev-list", "--count", UNPUSHED]);
        let Some(count) = Some(&ahead)
            .filter(|a| a.code == 0)
            .and_then(|a| trimmed(a).parse::<i64>().ok())
        else {
            o.error = Some(format!("git rev-list failed: {}", tail(&ahead)));
            return o;
        };
        if count > 0 {
            let alone = git(&["rev-list", "--first-parent", "--count", UNPUSHED]);
            if alone.code == 0 && trimmed(&alone) == "1" {
                if let Some(mine) = self.own_merge(e, LOCAL_MAIN, &git) {
                    o.merged = true;
                    o.commit = Some(mine);
                    o.error = Some(format!(
                        "merged locally, not pushed: main already holds the merge of {} from an earlier try, and nothing here proves its check passed. Undo the merge, then merge again.",
                        e.branch
                    ));
                    return o;
                }
            }
            o.error = Some(format!(
                "main has {count} commit(s) that are not on origin; push or undo them yourself"
            ));
            return o;
        }

        let refspec = format!("+refs/heads/{}:{reference}", e.branch);
        let fetch_branch = runner.run(
            &self
                .git_call(
                    &path,
                    &["fetch", "--no-tags", &machine_url, &refspec],
                    FETCH_TIMEOUT,
                )
                .env("GIT_SSH_COMMAND", shell::GIT_SSH_COMMAND),
        );
        if fetch_branch.code != 0 {
            o.error = Some(format!(
                "fetch from {} failed: {}",
                machine.name,
                tail(&fetch_branch)
            ));
            return o;
        }
        o.fetched = true;

        let tip = git(&["rev-parse", &reference]);
        if tip.code != 0 || e.commit.is_empty() || trimmed(&tip) != e.commit {
            o.error = Some("branch moved on the machine; refresh".into());
            return o;
        }

        let pre_head = git(&["rev-parse", "HEAD"]);
        let pre = trimmed(&pre_head);
        if pre_head.code != 0 || pre.is_empty() {
            o.error = Some(format!("git rev-parse HEAD failed: {}", tail(&pre_head)));
            return o;
        }
        let message = format!("Merge {} ({}#{})", e.branch, repo.github, e.issue);
        let merge = git(&["merge", "--no-ff", "-m", &message, &e.commit]);
        if merge.code != 0 {
            let started = git(&["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
            if started.code != 0 {
                o.error = Some(format!("merge failed: {}", tail(&merge)));
                return o;
            }
            let diff = git(&["diff", "--name-only", "--diff-filter=U"]);
            let files: Vec<String> = diff
                .out
                .split(['\n', '\r'])
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
            let abort = git(&["merge", "--abort"]);
            if abort.code != 0 {
                o.error = Some(format!(
                    "merge could not be aborted in {path}; fix it by hand"
                ));
                return o;
            }
            if diff.code != 0 {
                o.error = Some(format!(
                    "merge failed, and the conflicting files could not be listed: {}",
                    tail(&merge)
                ));
                return o;
            }
            if files.is_empty() {
                o.error = Some(format!("merge failed: {}", tail(&merge)));
                return o;
            }
            let list = files.join(", ");
            o.conflict_files = files;
            let mut error = format!("merge conflict in {list}");
            let note = format!(
                "notchling: merging {} into main conflicts in {list}. Needs you.",
                e.branch
            );
            let comment = runner.run(&self.gh_call(&[
                "issue",
                "comment",
                &issue,
                "-R",
                &repo.github,
                "--body",
                &note,
            ]));
            let label = runner.run(&self.gh_call(&[
                "issue",
                "edit",
                &issue,
                "-R",
                &repo.github,
                "--remove-label",
                "sd:ready",
                "--add-label",
                "sd:needs-you",
            ]));
            if comment.code != 0 || label.code != 0 {
                let which = if comment.code != 0 { &comment } else { &label };
                error.push_str(&format!(
                    " (the issue could not be updated: {})",
                    tail(which)
                ));
            }
            o.error = Some(error);
            return o;
        }
        let head = git(&["rev-parse", "HEAD"]);
        let sha = trimmed(&head);
        if head.code != 0 || sha.is_empty() {
            o.error = Some(format!("git rev-parse HEAD failed: {}", tail(&head)));
            return o;
        }
        if sha == pre {
            o.error = Some(format!("{} is already in main; nothing to merge", e.branch));
            return o;
        }
        o.merged = true;
        o.commit = Some(sha.clone());

        let (check_argv, check_env) = self.check_invocation(&repo.check_command);
        let mut check_call = Call::secs(check_argv, CHECK_TIMEOUT).cwd(path.clone().into());
        check_call.env = check_env;
        let check = runner.run(&check_call);
        if check.code != 0 {
            if let Some(changed) = self.change_after_check(&sha, &path, runner, false) {
                o.error = Some(changed);
                return o;
            }
            let reset = git(&["reset", "--keep", &pre]);
            if reset.code == 0 {
                o.error = Some(format!("check failed: {}", tail(&check)));
                o.merged = false;
                o.commit = None;
            } else {
                o.error = Some(format!(
                    "check failed and main could not be put back; fix it by hand in {path} (check: {}; reset: {})",
                    tail(&check),
                    tail(&reset)
                ));
            }
            return o;
        }
        if let Some(changed) = self.change_after_check(&sha, &path, runner, true) {
            o.error = Some(changed);
            return o;
        }
        o.checked = true;

        self.push_and_close(o, e, repo, &sha, &path, runner)
    }
}

pub(super) fn refused_issue(issue: i64) -> Option<String> {
    (issue <= 0).then(|| format!("\"{issue}\" is not an issue number"))
}

pub(super) fn worktree_url(machine: &Machine, path: &str) -> Result<String, String> {
    let url = shell::git_url(machine, path).ok_or_else(|| shell::not_absolute(path))?;
    if !shell::is_safe_machine_path(path) {
        return Err(format!(
            "the machine's worktree path \"{}\" has characters Notchling does not pass to a command",
            path.escape_debug()
        ));
    }
    Ok(url)
}

pub(super) fn trimmed(r: &CommandResult) -> String {
    r.out.trim().to_string()
}

pub(super) fn tail(r: &CommandResult) -> String {
    if r.code == 124 {
        return "timed out".into();
    }
    let text = format!("{}\n{}", r.err, r.out);
    let lines: Vec<&str> = text
        .trim()
        .split(['\n', '\r'])
        .filter(|l| !l.is_empty())
        .collect();
    let skip = lines.len().saturating_sub(20);
    let joined = lines[skip..].join("\n");
    let count = joined.chars().count();
    if count > 1500 {
        let cut: String = joined.chars().skip(count - 1500).collect();
        format!("…{cut}")
    } else {
        joined
    }
}
