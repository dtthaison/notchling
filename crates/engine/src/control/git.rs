//! The working copy a session runs in, for the inspector: branch, how far it is from its upstream,
//! the last commit and what changed (`GitInfo`). Parsing is pure; [`plan_git_status`] and [`read`]
//! run git through a [`Launcher`] with `--no-optional-locks`, so the session's own git never waits.

use super::launcher::Launcher;
use super::plan::Command;
use crate::sessions::lanes::standardize;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// "M" modified, "A" added, "D" deleted, "R" renamed, "?" untracked, "U" conflicted.
    pub status: String,
    pub path: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

/// A local branch, or a remote branch that has no local copy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub is_remote: bool,
    pub is_current: bool,
    pub ahead: u64,
    pub behind: u64,
    /// The branch's upstream is gone from the remote.
    pub gone: bool,
    pub has_upstream: bool,
    pub age: String,
    /// Folder of another working copy that has this branch checked out.
    pub worktree: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub last_commit: Option<String>,
    pub last_commit_age: Option<String>,
    pub changes: Vec<Change>,
    pub branches: Vec<Branch>,
}

/// The `for-each-ref` format `parse_branches` reads.
pub const BRANCH_FORMAT: &str = "%(HEAD)%09%(refname)%09%(upstream:short)%09%(upstream:track)%09%(committerdate:relative)%09%(worktreepath)";

/// "ahead 2, behind 1" counts, into `(ahead, behind)`.
fn counts(text: &str) -> (u64, u64) {
    let (mut ahead, mut behind) = (0, 0);
    for part in text.split(", ") {
        let words: Vec<&str> = part.split(' ').filter(|w| !w.is_empty()).collect();
        if words.len() != 2 {
            continue;
        }
        let Ok(n) = words[1].parse::<u64>() else {
            continue;
        };
        match words[0] {
            "ahead" => ahead = n,
            "behind" => behind = n,
            _ => {}
        }
    }
    (ahead, behind)
}

/// `git for-each-ref` output in [`BRANCH_FORMAT`], newest commit first. Local branches all show; a
/// remote branch shows only when no local branch has its name.
pub fn parse_branches(text: &str, folder: &str) -> Vec<Branch> {
    let (mut local, mut remote): (Vec<Branch>, Vec<Branch>) = (Vec::new(), Vec::new());
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 6 {
            continue;
        }
        let reference = c[1];
        if let Some(name) = reference.strip_prefix("refs/heads/") {
            let is_current = c[0] == "*";
            let (ahead, behind) = counts(c[3].trim_matches(|ch| ch == '[' || ch == ']'));
            let worktree = c[5];
            local.push(Branch {
                name: name.to_string(),
                is_current,
                age: c[4].to_string(),
                has_upstream: !c[2].is_empty(),
                gone: c[3] == "[gone]",
                ahead,
                behind,
                worktree: (!worktree.is_empty() && !is_current && worktree != folder)
                    .then(|| worktree.to_string()),
                ..Branch::default()
            });
        } else if let Some(short) = reference.strip_prefix("refs/remotes/") {
            if short.ends_with("/HEAD") || !short.contains('/') {
                continue;
            }
            remote.push(Branch {
                name: short.to_string(),
                is_remote: true,
                age: c[4].to_string(),
                ..Branch::default()
            });
        }
    }
    let names: HashSet<&str> = local.iter().map(|b| b.name.as_str()).collect();
    let keep: Vec<bool> = remote
        .iter()
        .map(|b| {
            let without_remote = b.name.split_once('/').map_or("", |(_, rest)| rest);
            !names.contains(without_remote)
        })
        .collect();
    let remote = remote
        .into_iter()
        .zip(keep)
        .filter_map(|(b, k)| k.then_some(b));
    let mut all = local;
    all.extend(remote);
    all
}

/// `git status --porcelain=v1 --branch` output.
pub fn parse_status(text: &str) -> GitInfo {
    let mut info = GitInfo::default();
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        if let Some(branch) = line.strip_prefix("## ") {
            parse_branch(branch, &mut info);
            continue;
        }
        if line.chars().count() <= 3 {
            continue;
        }
        let xy: String = line.chars().take(2).collect();
        let mut path: String = line.chars().skip(3).collect();
        if let Some((_, after)) = path.split_once(" -> ") {
            path = after.to_string();
        }
        if path.chars().count() > 1 && path.starts_with('"') && path.ends_with('"') {
            path = path[1..path.len() - 1].to_string();
        }
        info.changes.push(Change {
            status: status_letter(&xy).to_string(),
            path,
            ..Change::default()
        });
    }
    info
}

/// "main...origin/main [ahead 2, behind 1]", "HEAD (no branch)", "No commits yet on main".
fn parse_branch(text: &str, info: &mut GitInfo) {
    if let Some(name) = text.strip_prefix("No commits yet on ") {
        info.branch = Some(name.to_string());
        return;
    }
    if text.starts_with("HEAD (no branch)") {
        info.branch = Some("detached HEAD".to_string());
        return;
    }
    let (rest, count_text) = match text.split_once(" [") {
        Some((rest, tail)) => (rest, tail.trim_matches(']')),
        None => (text, ""),
    };
    let mut parts = rest.split("...");
    info.branch = parts.next().map(String::from);
    info.upstream = parts.next().map(String::from);
    let (ahead, behind) = counts(count_text);
    info.ahead = ahead;
    info.behind = behind;
}

fn status_letter(xy: &str) -> &'static str {
    if xy == "??" {
        return "?";
    }
    let x = xy.chars().next().unwrap_or(' ');
    let y = xy.chars().last().unwrap_or(' ');
    if x == 'U' || y == 'U' || xy == "AA" || xy == "DD" {
        return "U";
    }
    for (letters, answer) in [("RC", "R"), ("D", "D"), ("A", "A")] {
        if letters.contains(x) || letters.contains(y) {
            return answer;
        }
    }
    "M"
}

impl GitInfo {
    /// `git diff --numstat HEAD` output: added and removed lines per path ("-" for binary files).
    pub fn apply_numstat(&mut self, text: &str) {
        let mut counts: HashMap<String, (Option<u64>, Option<u64>)> = HashMap::new();
        for line in text.split('\n') {
            let cols: Vec<&str> = line.splitn(3, '\t').collect();
            if cols.len() != 3 {
                continue;
            }
            counts.insert(
                renamed_path(cols[2]),
                (cols[0].parse().ok(), cols[1].parse().ok()),
            );
        }
        for change in &mut self.changes {
            if let Some((added, removed)) = counts.get(&change.path) {
                change.added = *added;
                change.removed = *removed;
            }
        }
    }

    /// The last commit from `git log -1 --format=%h %s%x09%cr`.
    pub fn apply_log(&mut self, text: &str) {
        let trimmed = text.trim();
        let mut cols = trimmed.split('\t');
        self.last_commit = cols.next().filter(|c| !c.is_empty()).map(String::from);
        self.last_commit_age = cols.next().map(String::from);
    }
}

/// A rename reads "dir/{old => new}/file" or "old => new": the new path.
fn renamed_path(path: &str) -> String {
    let Some(arrow) = path.find(" => ") else {
        return path.to_string();
    };
    let after = arrow + " => ".len();
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        if open < close && after <= close {
            return format!(
                "{}{}{}",
                &path[..open],
                &path[after..close],
                &path[close + 1..]
            );
        }
    }
    path[after..].to_string()
}

fn git(folder: &str, args: &[&str]) -> Command {
    let mut argv = vec![
        "git",
        "-c",
        "core.quotepath=off",
        "--no-optional-locks",
        "-C",
        folder,
    ];
    argv.extend(args);
    Command::new(&argv)
}

/// The four commands `GitInfo.read` runs, in its order: status, numstat, log, branches. Never takes
/// the index lock.
pub fn plan_git_status(folder: &str) -> Vec<Command> {
    let format = format!("--format={BRANCH_FORMAT}");
    vec![
        git(
            folder,
            &[
                "status",
                "--porcelain=v1",
                "--branch",
                "--untracked-files=all",
            ],
        ),
        git(folder, &["diff", "--numstat", "HEAD"]),
        git(folder, &["log", "-1", "--format=%h %s%x09%cr"]),
        git(
            folder,
            &[
                "for-each-ref",
                "--sort=-committerdate",
                &format,
                "refs/heads",
                "refs/remotes",
            ],
        ),
    ]
}

/// Reads the folder's working copy; `None` when it is not one (or git is not there).
pub fn read(folder: &str, launcher: &dyn Launcher) -> Option<GitInfo> {
    if folder.is_empty() {
        return None;
    }
    let commands = plan_git_status(folder);
    let run = |command: &Command| {
        launcher
            .run(command, Duration::from_secs(30))
            .ok()
            .filter(|out| out.code == 0)
            .map(|out| out.output)
    };
    let mut info = parse_status(&run(&commands[0])?);
    if !info.changes.is_empty() {
        if let Some(numstat) = run(&commands[1]) {
            info.apply_numstat(&numstat);
        }
    }
    if let Some(log) = run(&commands[2]) {
        info.apply_log(&log);
    }
    if let Some(refs) = run(&commands[3]) {
        info.branches = parse_branches(&refs, &standardize(folder));
    }
    Some(info)
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
