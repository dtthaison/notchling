use super::repo_state::{Movable, RepoFacts};
use crate::cluster::id::{NodeId, Tool};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_FILE: u64 = 100 * 1024 * 1024;

pub const EXCLUDED_DIRS: &[&str] = &["cluster/grants", "cluster/logins", "cluster/incoming"];

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

pub fn excluded(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    for dir in EXCLUDED_DIRS {
        let want: Vec<_> = Path::new(dir).components().collect();
        if parts.windows(want.len()).any(|window| window == want) {
            return true;
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) => meta.is_file() && meta.len() > MAX_FILE,
        Err(_) => false,
    }
}
