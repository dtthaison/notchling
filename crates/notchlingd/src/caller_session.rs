use crate::vault_caller::Processes;
use notchling_engine::sessions::{config_dirs, Roots};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const LINEAGE_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionHit {
    pub session_id: String,
    pub claude_pid: u32,
    pub config_dir: PathBuf,
    pub chain: Vec<u32>,
}

pub struct CallerSession {
    roots: Roots,
    procs: Arc<dyn Processes>,
}

impl CallerSession {
    pub fn new(roots: Roots, procs: Arc<dyn Processes>) -> CallerSession {
        CallerSession { roots, procs }
    }

    pub fn session_of(&self, pid: u32) -> Option<SessionHit> {
        find_session(&self.roots, &|p| self.procs.parent(p), pid)
    }
}

pub const CLAUDE_PROCESS: &str = "claude";

pub fn is_claude(procs: &dyn Processes, hit: &SessionHit) -> bool {
    procs.name(hit.claude_pid).as_deref() == Some(CLAUDE_PROCESS)
}

pub fn trusted_in(
    roots: Roots,
    procs: Arc<dyn Processes>,
) -> Arc<dyn Fn(u32) -> Option<String> + Send + Sync> {
    let finder = CallerSession::new(roots, procs.clone());
    Arc::new(move |pid| {
        finder
            .session_of(pid)
            .filter(|hit| is_claude(procs.as_ref(), hit))
            .map(|hit| hit.session_id)
    })
}

pub(crate) fn find_session(
    roots: &Roots,
    parent_of: &dyn Fn(u32) -> Option<u32>,
    pid: u32,
) -> Option<SessionHit> {
    let lanes = config_dirs(roots);
    let mut chain = Vec::new();
    let mut next = parent_of(pid);
    for _ in 0..LINEAGE_DEPTH {
        let p = next.filter(|p| *p > 1)?;
        chain.push(p);
        let found = lanes.iter().find_map(|lane| {
            let file = lane.dir.join("sessions").join(format!("{p}.json"));
            session_id_in(&file).map(|id| (id, lane.dir.clone()))
        });
        if let Some((session_id, config_dir)) = found {
            return Some(SessionHit {
                session_id,
                claude_pid: p,
                config_dir,
                chain,
            });
        }
        next = parent_of(p);
    }
    None
}

pub(crate) fn session_id_in(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
        .and_then(|s| {
            s.get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|id| !id.is_empty())
}
