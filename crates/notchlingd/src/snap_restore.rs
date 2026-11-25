use crate::cluster_workspace::{command_for, command_with};
use crate::snap_repo::read_repo;
pub use crate::snap_restore_fs::DbHooks;
use crate::snap_restore_fs::{
    append_tail, make_dirs, merge_devices, merge_index, move_aside, put_dir, put_file, real_folder,
    repo_place, restore_db, stage_db,
};
use notchling_engine::cluster::restore::{
    blank_filters, drivers_probe, git_calls, origin_key, plan_restore, PathState, RestoreCtx,
    RestoreStep,
};
use notchling_engine::cluster::snapshot::Manifest;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::workspace::GitCall;
use notchling_engine::cluster::NodeId;
use notchling_engine::control::{Command, LaunchOutput, Launcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
    pub login_path: &'a [PathBuf],
    pub db: &'a DbHooks<'a>,
    pub set: Option<&'a str>,
    pub stage: Option<&'a Path>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub set: String,
    pub steps: usize,
    pub sessions: Vec<String>,
}

pub fn probe_state(git: &dyn Launcher, home: &Path, path: &Path) -> PathState {
    if repo_place(home, path, None, false).is_err() {
        return PathState::Foreign;
    }
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PathState::Missing,
        Ok(meta) if meta.is_dir() => {
            let empty = std::fs::read_dir(path).is_ok_and(|mut each| each.next().is_none());
            if empty {
                return PathState::Missing;
            }
            let facts = match read_repo(path, git) {
                Ok(Some(facts)) => facts,
                Ok(None) => return PathState::Foreign,
                Err(_) => return PathState::Changed,
            };
            let same = std::fs::canonicalize(path).is_ok_and(|real| real == facts.root);
            if !same {
                return PathState::Foreign;
            }
            if facts.dirty || facts.untracked {
                return PathState::Changed;
            }
            PathState::Clean {
                branch: facts.branch.unwrap_or_else(|| "(detached)".to_owned()),
                head: facts.head,
            }
        }
        _ => PathState::Foreign,
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

fn inherited() -> impl Iterator<Item = String> {
    std::env::vars_os().filter_map(|(key, _)| key.into_string().ok())
}

fn filter_names(git: &dyn Launcher, call: &GitCall) -> Result<String, String> {
    let cwd = Path::new(&call.cwd);
    if !cwd.is_dir() {
        return Ok(String::new());
    }
    let probe = drivers_probe(cwd);
    let out = git
        .run(
            &command_for(&probe, inherited()),
            Duration::from_secs(probe.timeout_secs),
        )
        .map_err(|e| format!("git {e}"))?;
    if out.code != 0 && out.code != 1 {
        return Err(format!("git config exited {}", out.code));
    }
    Ok(out.output)
}

fn run_git(git: &dyn Launcher, calls: &[GitCall]) -> Result<(), String> {
    for call in calls {
        let names = filter_names(git, call)?;
        let argv = blank_filters(call, &names)?;
        let out = git
            .run(
                &command_with(&argv, inherited()),
                Duration::from_secs(call.timeout_secs),
            )
            .map_err(|e| format!("git {e}"))?;
        if out.code != 0 && !call.allow_failure {
            return Err(failure(call, &out));
        }
    }
    Ok(())
}

fn verify_main(req: &Request, main: &Path, origin: &str) -> Result<(), String> {
    repo_place(req.home, main, None, false)?;
    real_folder(req.home, main)?;
    let shown = main.display();
    let facts = read_repo(main, req.read)?
        .ok_or_else(|| format!("{shown} isn't a repository on {}", req.node))?;
    let real = std::fs::canonicalize(main).map_err(|e| format!("{shown}: {e}"))?;
    if real != facts.root {
        return Err(format!("{shown} isn't the top of a repository"));
    }
    if facts.main_repo.is_some() {
        return Err(format!(
            "{shown} is a linked worktree, not a main repository"
        ));
    }
    if facts.origin.as_deref().map(origin_key) != Some(origin_key(origin)) {
        return Err(format!(
            "{shown} on {} has a different remote than the set's",
            req.node
        ));
    }
    Ok(())
}

pub fn run_step(step: &RestoreStep, req: &Request) -> Result<(), String> {
    let home = req.home;
    match step {
        RestoreStep::CloneRepo { path, .. } => {
            repo_place(home, path, None, true)?;
            if let Some(parent) = path.parent() {
                make_dirs(home, parent)?;
            }
            run_git(req.git, &git_calls(step))
        }
        RestoreStep::FastForward { path, .. } => {
            repo_place(home, path, None, false)?;
            real_folder(home, path)?;
            run_git(req.git, &git_calls(step))
        }
        RestoreStep::AddWorktree {
            main, origin, path, ..
        } => {
            repo_place(home, path, Some(main), true)?;
            verify_main(req, main, origin)?;
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
        RestoreStep::RestoreDb { from, to, sha256 } => match req.stage {
            Some(stage) => stage_db(home, from, stage, sha256),
            None => restore_db(home, from, to, sha256, req.db),
        },
        RestoreStep::MergeDevices { from, to, sha256 } => merge_devices(home, from, to, sha256),
        RestoreStep::MergeIndex {
            from,
            to,
            session,
            sha256,
        } => merge_index(home, from, to, session, sha256),
    }
}

const LOGIN_PATH_WAIT: Duration = Duration::from_secs(3);
const LOGIN_PATH_MAX: usize = 64 * 1024;
const LOGIN_PATH_MARK: &str = "NOTCHLING_LOGIN_PATH=";

pub fn login_path(launcher: &dyn Launcher, shell: Option<&str>) -> Vec<PathBuf> {
    let Some(shell) = shell.filter(|s| Path::new(s).is_absolute()) else {
        return Vec::new();
    };
    let script = format!("printf '\\n{LOGIN_PATH_MARK}%s\\n' \"$PATH\"");
    let command = Command::new(&[shell, "-lc", &script]);
    let Ok((out, false)) = launcher.run_capped(&command, LOGIN_PATH_WAIT, LOGIN_PATH_MAX) else {
        return Vec::new();
    };
    if out.code != 0 {
        return Vec::new();
    }
    out.output
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(LOGIN_PATH_MARK))
        .map(|path| {
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

pub fn chosen_set(
    cluster: &Path,
    primary: &NodeId,
    set: Option<&str>,
    node: &str,
) -> Result<Manifest, String> {
    let manifest = StandbyStore::new(cluster, primary)
        .current()
        .ok_or_else(|| "There is no set to restore from".to_owned())?;
    match set {
        Some(want) if manifest.set != want => Err(format!(
            "{node} doesn't hold the final snapshot {want} complete"
        )),
        _ => Ok(manifest),
    }
}

pub fn restore(req: &Request) -> Result<Report, String> {
    let manifest = chosen_set(req.cluster, req.primary, req.set, req.node)?;
    let mut path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    path_dirs.extend(req.login_path.iter().cloned());
    let ctx = RestoreCtx {
        cluster: req.cluster,
        home: req.home,
        node: req.node,
        aside: req.aside,
        shared: req.shared,
        path_dirs: &path_dirs,
    };
    let probe = |path: &Path| probe_state(req.read, req.home, path);
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
