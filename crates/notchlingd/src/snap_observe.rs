use crate::cluster_main::{agents_of, is_home};
use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::LocalRows;
use crate::snap_ready::{watch_step, ReadyWatch};
use crate::snap_send::SnapSender;
use notchling_engine::cluster::snapshot::{Observed, SessionState};
use notchling_engine::sessions::{Agent, AgentGroup};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub const EVERY: Duration = Duration::from_secs(2);
const MAX_UP: usize = 64;
const MAX_REFS: usize = 4_000;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct Known {
    pub cwd: String,
    pub transcript: Option<String>,
}

fn lock_name(name: &str) -> bool {
    name.ends_with(".lock")
}

fn has_lock(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_str().is_some_and(lock_name)
    })
}

fn ref_lock(dir: &Path, depth: usize, seen: &mut usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        *seen += 1;
        if *seen > MAX_REFS {
            return false;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        if kind.is_file() && name.to_str().is_some_and(lock_name) {
            return true;
        }
        if kind.is_dir() && ref_lock(&entry.path(), depth + 1, seen) {
            return true;
        }
    }
    false
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn resolve(base: &Path, text: &str) -> PathBuf {
    let path = Path::new(text.trim());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn git_dirs(folder: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut at = Some(folder);
    for _ in 0..MAX_UP {
        let here = at?;
        let dot = here.join(".git");
        match std::fs::symlink_metadata(&dot) {
            Ok(meta) if meta.is_dir() => return Some((dot.clone(), dot)),
            Ok(meta) if meta.is_file() => {
                let text = read_small(&dot)?;
                let target = text.trim().strip_prefix("gitdir:")?;
                let git_dir = resolve(here, target);
                let common = read_small(&git_dir.join("commondir"))
                    .map(|c| resolve(&git_dir, &c))
                    .unwrap_or_else(|| git_dir.clone());
                return Some((git_dir, common));
            }
            _ => {}
        }
        at = here.parent();
    }
    None
}

pub fn git_locked(folder: &Path) -> bool {
    let Some((git_dir, common)) = git_dirs(folder) else {
        return false;
    };
    if has_lock(&git_dir) || has_lock(&common) {
        return true;
    }
    let mut seen = 0;
    ref_lock(&common.join("refs"), 0, &mut seen)
}

pub fn modified_ms(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let at = meta.modified().ok()?;
    let since = at.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

pub struct Probes<'a> {
    pub locked: &'a dyn Fn(&Path) -> bool,
    pub written: &'a dyn Fn(&Path) -> Option<i64>,
}

pub fn observe(
    rows: &[Agent],
    main: &BTreeSet<String>,
    known: &mut BTreeMap<String, Known>,
    now_ms: i64,
    probes: &Probes,
) -> Observed {
    let mut observed = Observed {
        now_ms,
        ..Observed::default()
    };
    for agent in rows.iter().filter(|a| main.contains(&a.id)) {
        known.insert(
            agent.id.clone(),
            Known {
                cwd: agent.cwd.clone(),
                transcript: agent.transcript_path.clone(),
            },
        );
    }
    known.retain(|id, _| main.contains(id));
    let listed: BTreeSet<&str> = rows.iter().map(|a| a.id.as_str()).collect();
    for id in main {
        let working = rows
            .iter()
            .any(|a| a.id == *id && a.group != AgentGroup::Completed);
        let state = if !listed.contains(id.as_str()) {
            SessionState::Ended
        } else if working {
            SessionState::Working
        } else {
            SessionState::Idle
        };
        observed.session_state.insert(id.clone(), state);
        let Some(seen) = known.get(id) else {
            continue;
        };
        observed
            .git_locks
            .insert(id.clone(), (probes.locked)(Path::new(&seen.cwd)));
        let written = seen
            .transcript
            .as_deref()
            .and_then(|path| (probes.written)(Path::new(path)));
        if let Some(at) = written {
            observed.last_write_ms.insert(id.clone(), at);
        }
    }
    observed
}

pub fn main_ids(status: &Value) -> BTreeSet<String> {
    status
        .get("main")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn now_ms() -> i64 {
    crate::acting_ops::now_ms()
}

pub fn tick(node: &ClusterNode, rows: &dyn LocalRows, known: &mut BTreeMap<String, Known>) -> bool {
    let status = node.status();
    if !is_home(&status) {
        known.clear();
        return false;
    }
    let Ok(Some(rows)) = rows.complete_rows() else {
        return false;
    };
    let probes = Probes {
        locked: &git_locked,
        written: &modified_ms,
    };
    let observed = observe(
        &agents_of(rows),
        &main_ids(&status),
        known,
        now_ms(),
        &probes,
    );
    node.observe(observed);
    true
}

pub struct SnapObserver {
    stop: Mutex<Option<Sender<()>>>,
}

impl SnapObserver {
    pub fn spawn(
        node: Arc<ClusterNode>,
        rows: Arc<dyn LocalRows>,
        sender: Arc<SnapSender>,
        every: Duration,
    ) -> SnapObserver {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("snap-observe".into())
            .spawn(move || {
                let mut known = BTreeMap::new();
                let mut watch = ReadyWatch::default();
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    tick(&node, rows.as_ref(), &mut known);
                    watch_step(&node, &sender, &mut watch);
                    sender.tick();
                }
            });
        SnapObserver {
            stop: Mutex::new(spawned.ok().map(|_| stop)),
        }
    }

    pub fn close(&self) {
        self.stop.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

#[cfg(test)]
#[path = "snap_observe_tests.rs"]
mod tests;
