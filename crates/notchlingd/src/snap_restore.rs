use crate::cluster_workspace::command_for;
use crate::snap_repo::read_repo;
pub use crate::snap_restore_fs::DbHooks;
use crate::snap_restore_fs::{
    append_tail, make_dirs, merge_devices, merge_index, move_aside, put_dir, put_file, restore_db,
};
use notchling_engine::cluster::restore::{
    git_calls, plan_restore, PathState, RestoreCtx, RestoreStep,
};
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::workspace::GitCall;
use notchling_engine::cluster::NodeId;
use notchling_engine::control::{LaunchOutput, Launcher};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

pub struct Request<'a> {
    pub cluster: &'a Path,
    pub primary: &'a NodeId,
    pub home: &'a Path,
    pub node: &'a str,
    pub sessions: &'a BTreeSet<String>,
    pub aside: bool,
    pub shared: bool,
    pub git: &'a dyn Launcher,
    pub read: &'a dyn Launcher,
    pub db: &'a DbHooks<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub set: String,
    pub steps: usize,
    pub sessions: Vec<String>,
}

pub fn probe_state(git: &dyn Launcher, path: &Path) -> PathState {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PathState::Missing,
        Ok(meta) if meta.is_dir() => {
            let empty = std::fs::read_dir(path).is_ok_and(|mut each| each.next().is_none());
            if empty {
                return PathState::Missing;
            }
            let Ok(Some(facts)) = read_repo(path, git) else {
                return PathState::Changed;
            };
            let same = std::fs::canonicalize(path).is_ok_and(|real| real == facts.root);
            if !same || facts.dirty || facts.untracked {
                return PathState::Changed;
            }
            PathState::Clean {
                branch: facts.branch.unwrap_or_else(|| "(detached)".to_owned()),
                head: facts.head,
            }
        }
        _ => PathState::Changed,
    }
}

fn first_line(out: &LaunchOutput) -> String {
    let text = if out.stderr.trim().is_empty() {
        &out.output
    } else {
        &out.stderr
    };
    text.lines().next().unwrap_or("").trim().to_owned()
}

fn failure(call: &GitCall, out: &LaunchOutput) -> String {
    let all = format!("{}{}", out.stderr, out.output);
    let line = first_line(out);
    for (when, text) in &call.refusals {
        if when.is_empty() {
            return if line.is_empty() {
                text.clone()
            } else {
                format!("{text}: {line}")
            };
        }
        if all.contains(when.as_str()) {
            return text.clone();
        }
    }
    format!("git exited {}: {line}", out.code)
}

fn run_git(git: &dyn Launcher, calls: &[GitCall]) -> Result<(), String> {
    for call in calls {
        let command = command_for(
            call,
            std::env::vars_os().filter_map(|(key, _)| key.into_string().ok()),
        );
        let out = git
            .run(&command, Duration::from_secs(call.timeout_secs))
            .map_err(|e| format!("git {e}"))?;
        if out.code != 0 && !call.allow_failure {
            return Err(failure(call, &out));
        }
    }
    Ok(())
}

pub fn run_step(step: &RestoreStep, req: &Request) -> Result<(), String> {
    let home = req.home;
    match step {
        RestoreStep::CloneRepo { path, .. } => {
            if let Some(parent) = path.parent() {
                make_dirs(home, parent)?;
            }
            run_git(req.git, &git_calls(step))
        }
        RestoreStep::FastForward { path, .. } | RestoreStep::AddWorktree { path, .. } => {
            crate::snap_restore_fs::guard(home, path)?;
            if let Some(parent) = path.parent() {
                make_dirs(home, parent)?;
            }
            run_git(req.git, &git_calls(step))
        }
        RestoreStep::AppendTail {
            from,
            to,
            offset,
            end,
            prefix_sha256,
        } => append_tail(home, from, to, *offset, *end, prefix_sha256),
        RestoreStep::PutFile {
            from,
            to,
            sha256,
            len,
        } => put_file(home, from, to, sha256, *len),
        RestoreStep::PutDir {
            from,
            to,
            sha256,
            len,
        } => put_dir(home, from, to, sha256, *len),
        RestoreStep::MoveAside { path, to } => move_aside(home, path, to),
        RestoreStep::RestoreDb { from, to, sha256 } => restore_db(home, from, to, sha256, req.db),
        RestoreStep::MergeDevices { from, to, sha256 } => merge_devices(home, from, to, sha256),
        RestoreStep::MergeIndex {
            from,
            to,
            session,
            sha256,
        } => merge_index(home, from, to, session, sha256),
    }
}

pub fn restore(req: &Request) -> Result<Report, String> {
    let store = StandbyStore::new(req.cluster, req.primary);
    let manifest = store
        .current()
        .ok_or_else(|| "There is no set to restore from".to_owned())?;
    let ctx = RestoreCtx {
        cluster: req.cluster,
        home: req.home,
        node: req.node,
        aside: req.aside,
        shared: req.shared,
    };
    let probe = |path: &Path| probe_state(req.read, path);
    let steps = plan_restore(&manifest, &ctx, req.sessions, &probe)?;
    for step in &steps {
        run_step(step, req)?;
    }
    Ok(Report {
        set: manifest.set,
        steps: steps.len(),
        sessions: req.sessions.iter().cloned().collect(),
    })
}
