use notchling_engine::git::guard::clean_ref;
use notchling_engine::git::refs::RefRow;
use notchling_engine::git::worktrees::{WorktreeEntry, MAX_WORKTREES};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub const FRESH_READS: usize = 2;

fn forbidden(why: &str) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, why)
}

pub fn worktree_arg(raw: &str, listed: &[WorktreeEntry]) -> Result<PathBuf, ApiError> {
    let refuse = || forbidden("not a worktree of this repository");
    let path = Path::new(raw);
    if raw.is_empty() || raw.contains('\0') || !path.is_absolute() {
        return Err(refuse());
    }
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(refuse());
    }
    let real = std::fs::canonicalize(path).map_err(|_| refuse())?;
    let known = listed
        .iter()
        .filter(|entry| !entry.bare && !entry.prunable)
        .filter_map(|entry| std::fs::canonicalize(&entry.path).ok())
        .any(|candidate| candidate == real);
    if known {
        Ok(real)
    } else {
        Err(refuse())
    }
}

pub fn ref_arg<'a>(raw: &'a str, listed: &[RefRow]) -> Result<&'a str, ApiError> {
    let refuse = || forbidden("not a branch of this repository");
    let name = clean_ref(raw).map_err(|_| refuse())?;
    if !name.starts_with("refs/heads/") && !name.starts_with("refs/remotes/") {
        return Err(refuse());
    }
    if listed.iter().any(|row| row.refname == name) {
        Ok(name)
    } else {
        Err(refuse())
    }
}

pub fn fresh_args(raw: Option<&Value>, listed: &[WorktreeEntry]) -> Vec<PathBuf> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = Vec::new();
    for item in items.iter().take(MAX_WORKTREES) {
        if found.len() >= FRESH_READS {
            break;
        }
        let Some(text) = item.as_str() else { continue };
        let Ok(path) = worktree_arg(text, listed) else {
            continue;
        };
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}
