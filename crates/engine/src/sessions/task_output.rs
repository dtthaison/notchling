use std::fs;
use std::path::{Path, PathBuf};

const USER_DIRS: &str = "claude-";

pub fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn folders(of: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(of) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    At(PathBuf),
    Missing,
    Unsearchable,
}

pub fn find(tmp: &Path, id: &str) -> Found {
    if !plain_id(id) {
        return Found::Unsearchable;
    }
    let name = format!("{id}.output");
    let users: Vec<PathBuf> = folders(tmp)
        .into_iter()
        .filter(|user| {
            user.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(USER_DIRS))
        })
        .collect();
    if users.is_empty() {
        return Found::Unsearchable;
    }
    users
        .iter()
        .flat_map(|user| folders(user))
        .flat_map(|project| folders(&project))
        .map(|session| session.join("tasks").join(&name))
        .find(|candidate| candidate.symlink_metadata().is_ok())
        .map_or(Found::Missing, Found::At)
}

#[cfg(test)]
#[path = "task_output_tests.rs"]
mod tests;
