//! The rest of `MergePlan`: pushing again, taking a merge off `main`, the check's shell and the
//! branch-name checks. The merge itself is in [`super::merge`].

use std::collections::BTreeMap;

use super::merge::{
    refused_issue, tail, trimmed, MergeOutcome, MergePlan, FETCH_ORIGIN, FETCH_TIMEOUT,
    GIT_TIMEOUT, LOCAL_MAIN, UNPUSHED,
};
use super::registry::{Machine, MachineRepo};
use super::runner::{CommandResult, Runner};
use super::status::ReadyEntry;

const CHANGED: &str = "main changed during the check; nothing pushed";
const DIRTY: &str = "main has local changes made during the check; nothing pushed";

impl MergePlan {
    /// Repeats the push and the issue close alone, after a push failed. It pushes only the merge
    /// commit this branch made, sitting alone on top of origin/main with a clean tree on main;
    /// anything else is the user's. Only a merge whose check passed is ever offered here
    /// ([`MergeOutcome::can_retry_push`]).
    pub fn retry_push(
        &self,
        e: &ReadyEntry,
        _machine: &Machine,
        repo: &MachineRepo,
        runner: &dyn Runner,
    ) -> MergeOutcome {
        let path = match self.resolve_target(repo, runner) {
            Ok(p) => p,
            Err(why) => return MergeOutcome::failed(why),
        };
        let git = |args: &[&str]| runner.run(&self.git_call(&path, args, GIT_TIMEOUT));
        let refuse = |why: String| MergeOutcome::failed(format!("cannot push again: {why}"));
        if let Some(why) = refused_issue(e.issue) {
            return refuse(why);
        }
        let status = git(&["status", "--porcelain"]);
        if status.code != 0 || !trimmed(&status).is_empty() {
            return refuse(format!("main has local changes in {path}"));
        }
        let branch = git(&["symbolic-ref", "HEAD"]);
        if branch.code != 0 || trimmed(&branch) != LOCAL_MAIN {
            return refuse(format!("{path} is not on main"));
        }
        let second = git(&["rev-parse", "HEAD^2"]);
        if second.code != 0 || e.commit.is_empty() || trimmed(&second) != e.commit {
            return refuse(format!(
                "main's last commit is not the merge of {}",
                e.branch
            ));
        }
        // First parents only: the merged branch's own commits are not on origin either.
        let ahead = git(&["rev-list", "--first-parent", "--count", UNPUSHED]);
        if ahead.code != 0 || trimmed(&ahead) != "1" {
            return refuse("main is not exactly one merge ahead of origin".into());
        }
        let head = git(&["rev-parse", "HEAD"]);
        let sha = trimmed(&head);
        if head.code != 0 || sha.is_empty() {
            return refuse(format!("git rev-parse HEAD failed: {}", tail(&head)));
        }
        let o = MergeOutcome {
            fetched: true,
            merged: true,
            checked: true,
            commit: Some(sha.clone()),
            ..MergeOutcome::default()
        };
        self.push_and_close(o, e, repo, &sha, &path, runner)
    }

    /// Takes this item's merge off `main` again, while it sits there alone and unpushed: after a
    /// push origin refused, a change made during the check, or a quit during the check. `main`
    /// goes back to the commit the merge was made on, so the item can be merged again on top of
    /// what origin has now. Nothing else is touched: the tree must be clean, `main` checked out,
    /// `HEAD` a two-parent merge whose second parent is this item's commit, and exactly one
    /// first-parent commit ahead of origin once origin is fetched.
    /// Success is an outcome with no error and nothing merged.
    pub fn undo_merge(
        &self,
        e: &ReadyEntry,
        _machine: &Machine,
        repo: &MachineRepo,
        runner: &dyn Runner,
    ) -> MergeOutcome {
        let path = match self.resolve_target(repo, runner) {
            Ok(p) => p,
            Err(why) => return MergeOutcome::failed(why),
        };
        let git = |args: &[&str]| runner.run(&self.git_call(&path, args, GIT_TIMEOUT));
        let refuse = |why: String| MergeOutcome::failed(format!("cannot undo the merge: {why}"));
        let status = git(&["status", "--porcelain"]);
        if status.code != 0 || !trimmed(&status).is_empty() {
            return refuse(format!(
                "main has local changes in {path}; put them away first"
            ));
        }
        let branch = git(&["symbolic-ref", "HEAD"]);
        if branch.code != 0 || trimmed(&branch) != LOCAL_MAIN {
            return refuse(format!("{path} is not on main"));
        }
        if self.own_merge(e, "HEAD", &git).is_none() {
            return refuse(format!(
                "main's last commit is not a merge of {} alone",
                e.branch
            ));
        }
        // Whether the merge sits alone on top of origin is judged against what origin has now.
        let fetch = runner.run(&self.git_call(&path, &FETCH_ORIGIN, FETCH_TIMEOUT));
        if fetch.code != 0 {
            return refuse(format!("fetch origin main failed: {}", tail(&fetch)));
        }
        let ahead = git(&["rev-list", "--first-parent", "--count", UNPUSHED]);
        if ahead.code != 0 || trimmed(&ahead) != "1" {
            return refuse(
                "main holds other commits that are not on origin; undo them yourself".into(),
            );
        }
        let reset = git(&["reset", "--keep", "HEAD^1"]);
        if reset.code != 0 {
            return refuse(format!("git reset failed: {}", tail(&reset)));
        }
        MergeOutcome::default()
    }

    /// The sha of `revision` when it is a two-parent merge whose second parent is this item's
    /// commit, else `None`.
    pub(super) fn own_merge(
        &self,
        e: &ReadyEntry,
        revision: &str,
        git: &dyn Fn(&[&str]) -> CommandResult,
    ) -> Option<String> {
        if e.commit.is_empty() {
            return None;
        }
        let parents = git(&["rev-list", "--parents", "-n", "1", revision]);
        let out = trimmed(&parents);
        let parts: Vec<&str> = out.split(' ').filter(|p| !p.is_empty()).collect();
        (parents.code == 0 && parts.len() == 3 && parts[2] == e.commit)
            .then(|| parts[0].to_string())
    }

    /// Why the result of the check can no longer be used, or `None` while the target is still on
    /// `main` at the merge commit (and, before a push, clean: the check ran on that tree and the
    /// push sends that commit).
    pub(super) fn change_after_check(
        &self,
        sha: &str,
        path: &str,
        runner: &dyn Runner,
        require_clean: bool,
    ) -> Option<String> {
        let git = |args: &[&str]| runner.run(&self.git_call(path, args, GIT_TIMEOUT));
        let branch = git(&["symbolic-ref", "HEAD"]);
        if branch.code != 0 || trimmed(&branch) != LOCAL_MAIN {
            return Some(CHANGED.into());
        }
        let head = git(&["rev-parse", "HEAD"]);
        if head.code != 0 || trimmed(&head) != sha {
            return Some(CHANGED.into());
        }
        if require_clean {
            let status = git(&["status", "--porcelain"]);
            if status.code != 0 {
                return Some(CHANGED.into());
            }
            if !trimmed(&status).is_empty() {
                return Some(DIRTY.into());
            }
        }
        None
    }

    /// Pushes `main` (and nothing else, never forced) then closes the issue. A failed push keeps
    /// the merge commit.
    pub(super) fn push_and_close(
        &self,
        mut o: MergeOutcome,
        e: &ReadyEntry,
        repo: &MachineRepo,
        sha: &str,
        path: &str,
        runner: &dyn Runner,
    ) -> MergeOutcome {
        let spec = format!("{LOCAL_MAIN}:{LOCAL_MAIN}");
        let push = runner.run(&self.git_call(path, &["push", "origin", &spec], GIT_TIMEOUT));
        if push.code != 0 {
            o.error = Some(format!("push failed: {}", tail(&push)));
            return o;
        }
        o.pushed = true;
        let issue = e.issue.to_string();
        let comment = format!("notchling: merged in {sha}");
        let close = runner.run(&self.gh_call(&[
            "issue",
            "close",
            &issue,
            "-R",
            &repo.github,
            "--comment",
            &comment,
        ]));
        if close.code != 0 {
            o.error = Some(format!(
                "pushed, but the issue could not be closed: {}",
                tail(&close)
            ));
            return o;
        }
        o.closed = true;
        let label = runner.run(&self.gh_call(&[
            "issue",
            "edit",
            &issue,
            "-R",
            &repo.github,
            "--remove-label",
            "sd:ready",
        ]));
        if label.code != 0 {
            o.error = Some(format!(
                "closed, but sd:ready could not be removed: {}",
                tail(&label)
            ));
        }
        o
    }

    // The check's shell

    /// The user's login shell with the directories the repos' tools live in put in front of PATH.
    /// A menu bar app starts with a bare PATH, and a repo's `just` comes from mise and its `cargo`
    /// from `~/.cargo/bin`.
    pub fn check_invocation(&self, check_command: &str) -> (Vec<String>, Vec<(String, String)>) {
        MergePlan::check_invocation_with(
            check_command,
            &self.environment,
            &self.home,
            &is_executable,
        )
    }

    pub fn check_invocation_with(
        check_command: &str,
        environment: &BTreeMap<String, String>,
        home: &str,
        is_executable: &dyn Fn(&str) -> bool,
    ) -> (Vec<String>, Vec<(String, String)>) {
        let shell = environment
            .get("SHELL")
            .filter(|s| s.starts_with('/') && is_executable(s))
            .cloned()
            .unwrap_or_else(|| "/bin/zsh".into());
        let tools = [
            format!("{home}/.cargo/bin"),
            format!("{home}/.local/share/mise/shims"),
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
        ];
        let base = environment
            .get("PATH")
            .cloned()
            .unwrap_or_else(|| "/usr/bin:/bin:/usr/sbin:/sbin".into());
        let path = tools
            .iter()
            .cloned()
            .chain([base])
            .collect::<Vec<_>>()
            .join(":");
        (
            vec![shell, "-lc".into(), check_command.into()],
            vec![("PATH".into(), path)],
        )
    }

    // Branch names

    /// A branch name git must never be handed as an option or a refspec part: empty, starting
    /// with `-`, or holding a space or control character. Git's own rules (`check-ref-format`)
    /// judge the rest.
    pub fn refused_branch(branch: &str) -> Option<String> {
        let odd = branch.is_empty()
            || branch.starts_with('-')
            || branch
                .chars()
                .any(|c| (c as u32) <= 0x20 || c as u32 == 0x7f);
        odd.then(|| not_a_branch(branch))
    }

    /// `git check-ref-format --branch` must accept the name and print it back unchanged (it
    /// expands `@{-1}`).
    pub fn git_refuses_branch(
        &self,
        branch: &str,
        path: &str,
        runner: &dyn Runner,
    ) -> Option<String> {
        let r = runner.run(&self.git_call(
            path,
            &["check-ref-format", "--branch", branch],
            GIT_TIMEOUT,
        ));
        (r.code != 0 || trimmed(&r) != branch).then(|| not_a_branch(branch))
    }

    /// Both checks, for a caller that has the repo's folder and no other use for a target.
    pub fn branch_error(&self, branch: &str, path: &str, runner: &dyn Runner) -> Option<String> {
        if let Some(why) = MergePlan::refused_branch(branch) {
            return Some(why);
        }
        self.git_refuses_branch(branch, path, runner)
    }
}

fn not_a_branch(branch: &str) -> String {
    format!(
        "\"{}\" is not a branch name git accepts",
        branch.escape_debug()
    )
}

#[cfg(unix)]
fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// No execute bit off Unix: a file is as close as it gets.
#[cfg(not(unix))]
fn is_executable(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}
