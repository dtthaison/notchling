use super::plan::SessionEntry;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Db,
    File,
    Dir,
    Tail {
        from: u64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rewritten: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub kind: PartKind,
    pub path: PathBuf,
    pub sha256: String,
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub set: String,
    pub seq: u64,
    pub term: Term,
    pub home: NodeId,
    pub created_ms: i64,
    pub sessions: Vec<SessionEntry>,
    pub parts: Vec<Part>,
}

pub fn dir_digest(entries: &[(String, u64, String)]) -> String {
    let mut sorted: Vec<&(String, u64, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, len, sha256) in sorted {
        hasher.update(format!("{sha256} {len} {path}\n").as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn staged_path(set_dir: &Path, source: &Path) -> Option<PathBuf> {
    if !source.is_absolute() {
        return None;
    }
    let text = source.to_string_lossy();
    if text.split(['/', '\\']).any(|part| part == ".") {
        return None;
    }
    let mut target = set_dir.join("parts");
    for component in source.components() {
        match component {
            Component::Normal(name) => target.push(name),
            Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir | Component::ParentDir => return None,
        }
    }
    Some(target)
}
