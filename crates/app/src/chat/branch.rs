use std::sync::Arc;

use notchling_kit::Ink;
use serde_json::{json, Value};

use crate::acting::Request;

pub const STAT_OP: &str = "git.branch_diff_stat";
pub const FILE_OP: &str = "git.branch_diff_file";
pub const BRANCHES_OP: &str = "git.branches";

pub const LARGE_LINES: u64 = 2_000;
pub const LARGE_FILES: u64 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub from: Option<String>,
    pub added: Option<u64>,
    pub removed: Option<u64>,
    pub uncommitted: bool,
}

impl ChangedFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn folder(&self) -> &str {
        match self.path.rsplit_once('/') {
            Some((folder, _)) => folder,
            None => "",
        }
    }

    pub fn is_new(&self) -> bool {
        matches!(self.status.as_str(), "A" | "?")
    }

    pub fn name_ink(&self) -> Ink {
        if self.is_new() {
            Ink::Done
        } else if self.status == "D" {
            Ink::Expired
        } else {
            Ink::Text
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub added: u64,
    pub removed: u64,
    pub files: u64,
    pub base: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchModel {
    pub branch: String,
    pub base: String,
    pub merge_base: String,
    pub files: Vec<ChangedFile>,
    pub added: u64,
    pub removed: u64,
    pub file_count: u64,
    pub uncommitted_files: u64,
    pub truncated: bool,
}

impl BranchModel {
    pub fn summary(&self) -> Summary {
        Summary {
            added: self.added,
            removed: self.removed,
            files: self.file_count,
            base: self.base.clone(),
        }
    }

    pub fn is_large(&self) -> bool {
        self.added + self.removed > LARGE_LINES || self.file_count > LARGE_FILES
    }

    pub fn find(&self, path: &str) -> Option<&ChangedFile> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn counts_line(&self) -> String {
        let files = match self.file_count {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        match self.uncommitted_files {
            0 => files,
            n => format!("{files} \u{b7} {n} uncommitted"),
        }
    }
}

pub fn branch_name(reply: &Value) -> String {
    match (reply["detached"].as_bool(), reply["branch"].as_str()) {
        (Some(true), _) => match reply["head"].as_str() {
            Some(head) => format!("Detached at {head}"),
            None => "Detached HEAD".to_owned(),
        },
        (_, Some(name)) => name.to_owned(),
        _ => "No branch".to_owned(),
    }
}

pub fn parse_stat(reply: &Value) -> Option<BranchModel> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let base = reply["base"].as_str().filter(|b| !b.is_empty())?;
    let files = reply["files"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(ChangedFile {
                        path: row["path"].as_str()?.to_owned(),
                        status: row["status"].as_str()?.to_owned(),
                        from: row["from"].as_str().map(str::to_owned),
                        added: row["added"].as_u64(),
                        removed: row["removed"].as_u64(),
                        uncommitted: row["uncommitted"].as_bool().unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(BranchModel {
        branch: branch_name(reply),
        base: base.to_owned(),
        merge_base: reply["merge_base"].as_str().unwrap_or("").to_owned(),
        added: reply["added"].as_u64().unwrap_or(0),
        removed: reply["removed"].as_u64().unwrap_or(0),
        file_count: reply["file_count"].as_u64().unwrap_or(files.len() as u64),
        uncommitted_files: reply["uncommitted_files"].as_u64().unwrap_or(0),
        truncated: reply["truncated"].as_bool().unwrap_or(false),
        files,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branches {
    pub current: Option<String>,
    pub default: Option<String>,
    pub remote_default: Option<String>,
    pub local: Vec<String>,
}

impl Branches {
    pub fn choices(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |name: &str| {
            if !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
            }
        };
        if let Some(remote) = &self.remote_default {
            push(remote);
        }
        for name in &self.local {
            if self.current.as_deref() != Some(name.as_str()) {
                push(name);
            }
        }
        out
    }
}

pub fn parse_branches(reply: &Value) -> Option<Branches> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let text = |key: &str| reply[key].as_str().map(str::to_owned);
    Some(Branches {
        current: text("current"),
        default: text("default"),
        remote_default: text("remote_default"),
        local: reply["local"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn stat_request(cwd: &str, base: Option<&str>) -> Request {
    let mut args = json!({ "cwd": cwd });
    if let Some(base) = base {
        args["base"] = json!(base);
    }
    Request { op: STAT_OP, args }
}

pub fn file_request(cwd: &str, base: &str, path: &str, from: Option<&str>) -> Request {
    let mut args = json!({ "cwd": cwd, "base": base, "path": path });
    if let Some(from) = from {
        args["from"] = json!(from);
    }
    Request { op: FILE_OP, args }
}

pub fn branches_request(cwd: &str) -> Request {
    Request {
        op: BRANCHES_OP,
        args: json!({ "cwd": cwd }),
    }
}

pub fn matches(path: &str, query: &str) -> bool {
    let lower = path.to_lowercase();
    query
        .split_whitespace()
        .all(|term| lower.contains(&term.to_lowercase()))
}

pub fn visible(files: &[ChangedFile], query: &str) -> Vec<usize> {
    files
        .iter()
        .enumerate()
        .filter(|(_, f)| matches(&f.path, query))
        .map(|(i, _)| i)
        .collect()
}

pub fn preview_model() -> Arc<BranchModel> {
    let file =
        |path: &str, status: &str, from: Option<&str>, added, removed, uncommitted| ChangedFile {
            path: path.to_owned(),
            status: status.to_owned(),
            from: from.map(str::to_owned),
            added,
            removed,
            uncommitted,
        };
    let files = vec![
        file(
            "crates/app/src/chat/branch.rs",
            "A",
            None,
            Some(231),
            Some(0),
            true,
        ),
        file(
            "crates/app/src/chat/inspector.rs",
            "M",
            None,
            Some(14),
            Some(9),
            false,
        ),
        file(
            "crates/engine/src/git/branch.rs",
            "A",
            None,
            Some(188),
            Some(0),
            false,
        ),
        file(
            "crates/engine/src/git/read.rs",
            "M",
            None,
            Some(22),
            Some(31),
            false,
        ),
        file(
            "crates/engine/src/old_helper.rs",
            "D",
            None,
            Some(0),
            Some(48),
            false,
        ),
        file("docs/local-api-ops.md", "M", None, Some(31), Some(2), false),
        file(
            "docs/guide.md",
            "R",
            Some("docs/intro.md"),
            Some(3),
            Some(1),
            false,
        ),
        file("notes.md", "?", None, Some(12), Some(0), true),
    ];
    let sum = |pick: fn(&ChangedFile) -> Option<u64>| files.iter().filter_map(pick).sum();
    Arc::new(BranchModel {
        branch: "agent/branch-changes-panel".to_owned(),
        base: "origin/main".to_owned(),
        merge_base: "8e73a97".to_owned(),
        added: sum(|f| f.added),
        removed: sum(|f| f.removed),
        file_count: files.len() as u64,
        uncommitted_files: 2,
        truncated: false,
        files,
    })
}

pub fn preview_branches() -> Branches {
    Branches {
        current: Some("agent/branch-changes-panel".to_owned()),
        default: Some("origin/main".to_owned()),
        remote_default: Some("origin/main".to_owned()),
        local: vec![
            "agent/branch-changes-panel".to_owned(),
            "main".to_owned(),
            "agent/move-auto-trust".to_owned(),
        ],
    }
}

pub const PREVIEW_PATH: &str = "crates/app/src/chat/inspector.rs";

#[cfg(test)]
#[path = "branch_tests.rs"]
mod tests;
