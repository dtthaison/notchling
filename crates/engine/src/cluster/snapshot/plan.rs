use super::exclude::{excluded, excluded_dir, MAX_FILE};
use super::repo_state::{movable, Movable, RepoFacts};
use super::resume_set::{resume_set, ResumeSet};
use crate::bridge::net::devices::FILE_NAME as DEVICES;
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::main_sessions::MainSession;
use crate::cluster::persist::{CLUSTER_DB, CLUSTER_DIR};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const TIMER_MS: i64 = 300_000;
pub const QUIET_MS: i64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Working,
    Idle,
    Ended,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub now_ms: i64,
    pub session_state: BTreeMap<String, SessionState>,
    pub git_locks: BTreeMap<String, bool>,
    pub last_write_ms: BTreeMap<String, i64>,
    pub last_set_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    TurnEnd(String),
    Timer,
    Ready(NodeId),
    Final,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: String,
    pub tool: Tool,
    pub cwd: String,
    pub repo: Option<RepoFacts>,
    pub movable: Movable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetPlan {
    pub id: String,
    pub trigger: Trigger,
    pub sessions: Vec<SessionEntry>,
    pub tails: Vec<(PathBuf, u64)>,
    pub files: Vec<PathBuf>,
    pub dbs: Vec<PathBuf>,
}

pub fn quiet(now: &Observed) -> bool {
    let locked = now.git_locks.values().any(|locked| *locked);
    let written = now
        .last_write_ms
        .values()
        .any(|at| now.now_ms - *at < QUIET_MS);
    !locked && !written
}

fn turn_end(prev: &Observed, now: &Observed) -> Option<String> {
    now.session_state
        .iter()
        .find(|(id, state)| {
            matches!(state, SessionState::Idle | SessionState::Ended)
                && prev.session_state.get(*id) == Some(&SessionState::Working)
        })
        .map(|(id, _)| id.clone())
}

pub fn due(prev: &Observed, now: &Observed) -> Option<Trigger> {
    if !quiet(now) {
        return None;
    }
    if let Some(id) = turn_end(prev, now) {
        return Some(Trigger::TurnEnd(id));
    }
    let working = now
        .session_state
        .values()
        .any(|state| *state == SessionState::Working);
    let elapsed = now.last_set_ms.is_none_or(|at| now.now_ms - at >= TIMER_MS);
    (elapsed && !working).then_some(Trigger::Timer)
}

pub fn carry(prev: &Observed, now: &Observed) -> Observed {
    let mut next = now.clone();
    if quiet(now) {
        return next;
    }
    for (id, state) in &mut next.session_state {
        let was_working = prev.session_state.get(id) == Some(&SessionState::Working);
        if was_working && *state != SessionState::Working {
            *state = SessionState::Working;
        }
    }
    next
}

pub fn smallest_acked(targets: &[BTreeMap<PathBuf, u64>]) -> BTreeMap<PathBuf, u64> {
    let paths: BTreeSet<&PathBuf> = targets.iter().flat_map(BTreeMap::keys).collect();
    paths
        .into_iter()
        .map(|path| {
            let least = targets
                .iter()
                .map(|acked| acked.get(path).copied().unwrap_or(0))
                .min()
                .unwrap_or(0);
            (path.clone(), least)
        })
        .collect()
}

fn resolved(path: &Path) -> Option<PathBuf> {
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return Some(rest.iter().rev().fold(real, |acc, name| acc.join(name)));
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    }
}

fn forms(path: &Path) -> Vec<PathBuf> {
    let mut all = vec![path.to_path_buf()];
    all.extend(resolved(path));
    for ancestor in path.ancestors() {
        let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) else {
            continue;
        };
        let (Some(real), Ok(rest)) = (resolved(parent), path.strip_prefix(ancestor)) else {
            continue;
        };
        all.push(real.join(name).join(rest));
    }
    let mut seen = BTreeSet::new();
    all.retain(|form| seen.insert(form.clone()));
    all
}

pub(super) fn holds(root: &Path, path: &Path) -> bool {
    let roots = forms(root);
    forms(path).iter().any(|each| {
        roots
            .iter()
            .any(|r| each.starts_with(r) || r.starts_with(each))
    })
}

fn contains(path: &Path, root: &Path) -> bool {
    let paths = forms(path);
    forms(root)
        .iter()
        .any(|r| paths.iter().any(|each| r.starts_with(each)))
}

fn roots_of(facts: &RepoFacts) -> Vec<PathBuf> {
    let mut roots = vec![facts.root.clone()];
    roots.extend(facts.main_repo.clone());
    roots
}

fn bridge_devices(data: &Path) -> PathBuf {
    data.parent()
        .unwrap_or(data)
        .join("AgentConsole")
        .join("bridge")
        .join(DEVICES)
}

fn session_paths(set: &ResumeSet) -> impl Iterator<Item = &PathBuf> {
    set.transcripts
        .iter()
        .chain(set.files.iter())
        .chain(set.dirs.iter())
}

fn entry(m: &MainSession, repos: &BTreeMap<String, Option<RepoFacts>>) -> SessionEntry {
    let (repo, verdict) = match repos.get(&m.id) {
        None => (
            None,
            Movable::No(format!("{} couldn't be read as a repository", m.cwd)),
        ),
        Some(facts) => (facts.clone(), movable(facts.as_ref(), &m.cwd)),
    };
    SessionEntry {
        id: m.id.clone(),
        tool: m.tool,
        cwd: m.cwd.clone(),
        repo,
        movable: verdict,
    }
}

fn tail_len(path: &Path, from: u64) -> u64 {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.len().saturating_sub(from))
        .unwrap_or(0)
}

pub fn plan_set(
    main: &[MainSession],
    repos: &BTreeMap<String, Option<RepoFacts>>,
    home: &Path,
    data: &Path,
    seq: u64,
    trigger: Trigger,
    acked: &BTreeMap<PathBuf, u64>,
) -> SetPlan {
    let mut order: Vec<&MainSession> = main.iter().collect();
    order.sort_by(|a, b| a.id.cmp(&b.id));
    let sets: Vec<ResumeSet> = order.iter().map(|m| resume_set(m, home)).collect();
    let dbs = vec![data.join(CLUSTER_DIR).join(CLUSTER_DB)];
    let shared = [data.join("pages"), bridge_devices(data)];
    let mut sessions: Vec<SessionEntry> = order.iter().map(|m| entry(m, repos)).collect();

    let every: Vec<&Path> = sets
        .iter()
        .flat_map(session_paths)
        .chain(shared.iter())
        .chain(dbs.iter())
        .map(PathBuf::as_path)
        .collect();
    let mut dropped: Vec<(usize, Vec<PathBuf>)> = Vec::new();
    let mut repo_roots: Vec<PathBuf> = Vec::new();
    for (at, session) in sessions.iter_mut().enumerate() {
        let Some(facts) = session.repo.as_ref() else {
            continue;
        };
        let roots = roots_of(facts);
        repo_roots.extend(roots.iter().cloned());
        if !roots
            .iter()
            .any(|root| every.iter().any(|path| holds(root, path)))
        {
            continue;
        }
        let root = facts.root.clone();
        session.movable = Movable::No(format!("{} holds files a set carries", root.display()));
        session.repo = None;
        dropped.push((at, roots));
    }

    let mut tails: BTreeMap<PathBuf, u64> = BTreeMap::new();
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    for (at, set) in sets.iter().enumerate() {
        let inside = |path: &PathBuf| {
            dropped
                .iter()
                .any(|(who, roots)| *who == at && roots.iter().any(|root| holds(root, path)))
        };
        for path in set.transcripts.iter().filter(|p| !inside(p)) {
            let from = acked.get(path).copied().unwrap_or(0);
            if !excluded_dir(path) && tail_len(path, from) <= MAX_FILE {
                tails.insert(path.clone(), from);
            }
        }
        for path in set.files.iter().chain(&set.dirs).filter(|p| !inside(p)) {
            files.insert(path.clone());
        }
    }
    files.extend(shared);
    files.retain(|path| !repo_roots.iter().any(|root| contains(path, root)));
    SetPlan {
        id: format!("s-{seq}"),
        trigger,
        sessions,
        tails: tails.into_iter().collect(),
        files: files.into_iter().filter(|path| !excluded(path)).collect(),
        dbs,
    }
}
