use notchling_locale as t;
use std::sync::Arc;

use notchling_kit::Ink;
use serde_json::{json, Value};

use crate::acting::Request;

pub const STATUS_OP: &str = "git.status";
pub const DIFF_OP: &str = "git.diff";

pub const EVERY_MS: i64 = 5_000;
pub const EVENT_FLOOR_MS: i64 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFile {
    pub path: String,
    pub status: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

impl GitFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    pub fn folder(&self) -> &str {
        match self.path.rsplit_once('/') {
            Some((folder, _)) => folder,
            None => "",
        }
    }

    pub fn ink(&self) -> Ink {
        match self.status.as_str() {
            "A" => Ink::Done,
            "D" | "U" => Ink::Expired,
            "?" => Ink::TextFaint,
            _ => Ink::AccentText,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitModel {
    pub branch: String,
    pub files: Vec<GitFile>,
    pub truncated: bool,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub initial: bool,
    pub changed_at: Option<i64>,
}

pub fn tracking(upstream: &str, ahead: u64, behind: u64) -> String {
    match (ahead, behind) {
        (0, 0) => t::chat::git_up_to_date(upstream),
        (a, 0) => t::chat::git_ahead(upstream, &a.to_string()),
        (0, b) => t::chat::git_behind(upstream, &b.to_string()),
        (a, b) => t::chat::git_ahead_behind(upstream, &a.to_string(), &b.to_string()),
    }
}

pub fn parse_status(reply: &Value) -> Option<GitModel> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let branch = match (reply["detached"].as_bool(), reply["branch"].as_str()) {
        (Some(true), _) => match reply["head"].as_str() {
            Some(head) => t::chat::git_detached_at(head),
            None => t::chat::git_detached().to_owned(),
        },
        (_, Some(name)) => name.to_owned(),
        _ => t::chat::git_no_branch().to_owned(),
    };
    let files = reply["changes"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(GitFile {
                        path: row["path"].as_str()?.to_owned(),
                        status: row["status"].as_str()?.to_owned(),
                        added: row["added"].as_u64(),
                        removed: row["removed"].as_u64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(GitModel {
        branch,
        files,
        truncated: reply["truncated"].as_bool().unwrap_or(false),
        upstream: reply["upstream"].as_str().map(str::to_owned),
        ahead: reply["ahead"].as_u64().unwrap_or(0),
        behind: reply["behind"].as_u64().unwrap_or(0),
        initial: reply["initial"].as_bool().unwrap_or(false),
        changed_at: reply["changed_at"].as_i64(),
    })
}

pub fn status_request(cwd: &str) -> Request {
    Request {
        op: STATUS_OP,
        args: json!({ "cwd": cwd }),
    }
}

pub fn diff_request(cwd: &str, path: &str) -> Request {
    Request {
        op: DIFF_OP,
        args: json!({ "cwd": cwd, "path": path }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Opened,
    Folder,
    TurnEnded,
    Tick,
}

#[derive(Debug, Default)]
pub struct Gate {
    last_ms: Option<i64>,
    folder: Option<String>,
    inflight: bool,
}

impl Gate {
    pub fn ask(&mut self, why: Why, folder: Option<&str>, visible: bool, now_ms: i64) -> bool {
        let Some(folder) = folder.filter(|f| !f.is_empty()) else {
            return false;
        };
        if !visible || self.inflight {
            return false;
        }
        let since = self.last_ms.map_or(i64::MAX, |last| now_ms - last);
        let changed = self.folder.as_deref() != Some(folder);
        let go = match why {
            Why::Opened | Why::Folder => changed || since >= EVENT_FLOOR_MS,
            Why::TurnEnded => since >= EVENT_FLOOR_MS,
            Why::Tick => changed || since >= EVERY_MS,
        };
        if go {
            self.last_ms = Some(now_ms);
            self.folder = Some(folder.to_owned());
            self.inflight = true;
        }
        go
    }

    pub fn done(&mut self) {
        self.inflight = false;
    }

    pub fn reset(&mut self) {
        self.last_ms = None;
    }

    pub fn inflight(&self) -> bool {
        self.inflight
    }
}

pub fn diff_due(last_ms: Option<i64>, now_ms: i64) -> bool {
    last_ms.is_none_or(|last| now_ms - last >= EVERY_MS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Hunk,
    Meta,
    Context,
}

impl DiffKind {
    pub fn ink(self) -> Ink {
        match self {
            DiffKind::Added => Ink::Done,
            DiffKind::Removed => Ink::Expired,
            DiffKind::Hunk => Ink::AccentText,
            DiffKind::Meta => Ink::TextFaint,
            DiffKind::Context => Ink::Text,
        }
    }
}

pub fn runs(text: &str) -> Vec<(DiffKind, String)> {
    let mut out: Vec<(DiffKind, String)> = Vec::new();
    let mut in_hunk = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let kind = if bare.starts_with("diff --git") || bare.starts_with("diff --cc") {
            in_hunk = false;
            DiffKind::Meta
        } else if bare.starts_with("@@") {
            in_hunk = true;
            DiffKind::Hunk
        } else if !in_hunk {
            DiffKind::Meta
        } else {
            match bare.chars().next() {
                Some('+') => DiffKind::Added,
                Some('-') => DiffKind::Removed,
                Some('\\') => DiffKind::Meta,
                _ => DiffKind::Context,
            }
        };
        match out.last_mut() {
            Some((last, text)) if *last == kind => text.push_str(line),
            _ => out.push((kind, line.to_owned())),
        }
    }
    for (_, text) in &mut out {
        let kept = text.trim_end_matches(['\n', '\r']).len();
        text.truncate(kept);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffText {
    pub runs: Vec<(DiffKind, String)>,
    pub truncated: bool,
    pub untracked: bool,
}

pub const CHUNK_LINES: usize = 80;

pub fn chunks(diff: &DiffText) -> Vec<(Ink, String)> {
    let mut out: Vec<(Ink, String)> = Vec::new();
    for (kind, text) in &diff.runs {
        let lines: Vec<&str> = text.split('\n').collect();
        for chunk in lines.chunks(CHUNK_LINES) {
            out.push((kind.ink(), chunk.join("\n")));
        }
    }
    out
}

pub fn parse_diff(reply: &Value) -> Option<DiffText> {
    let text = reply["text"].as_str()?;
    Some(DiffText {
        runs: runs(text),
        truncated: reply["truncated"].as_bool().unwrap_or(false),
        untracked: reply["untracked"].as_bool().unwrap_or(false),
    })
}

pub fn preview_model() -> Arc<GitModel> {
    let file = |path: &str, status: &str, added, removed| GitFile {
        path: path.to_owned(),
        status: status.to_owned(),
        added,
        removed,
    };
    Arc::new(GitModel {
        branch: "agent/details-git".to_owned(),
        files: vec![
            file("crates/app/src/chat/inspector.rs", "M", Some(14), Some(9)),
            file("crates/app/src/chat/git.rs", "A", Some(212), Some(0)),
            file("crates/engine/src/old_helper.rs", "D", Some(0), Some(48)),
        ],
        truncated: false,
        upstream: Some("origin/agent-run-oss".to_owned()),
        ahead: 2,
        behind: 0,
        initial: false,
        changed_at: None,
    })
}

pub const PREVIEW_PATH: &str = "crates/app/src/chat/inspector.rs";

pub fn preview_diff() -> DiffText {
    let text = concat!(
        "diff --git a/crates/app/src/chat/inspector.rs b/crates/app/src/chat/inspector.rs\n",
        "index 3f2a9c1..8b7d0e4 100644\n",
        "--- a/crates/app/src/chat/inspector.rs\n",
        "+++ b/crates/app/src/chat/inspector.rs\n",
        "@@ -300,12 +300,6 @@ pub fn facts(conversation: &Conversation) -> Vec<(&'static str, String)> {\n",
        "     if let Some(branch) = &f.branch {\n",
        "         out.push((\"Branch\", branch.clone()));\n",
        "     }\n",
        "-    if let Some(model) = &f.model {\n",
        "-        out.push((\"Model\", model_name(model)));\n",
        "-    }\n",
        "-    if let Some(effort) = &f.effort {\n",
        "-        out.push((\"Effort\", effort.clone()));\n",
        "-    }\n",
        "     if f.turns > 0 {\n",
        "         out.push((\"Turns\", f.turns.to_string()));\n",
        "     }\n",
        "@@ -400,3 +394,4 @@ pub fn fact_icon(label: &str) -> Icon {\n",
        "         \"Turns\" => Icon::Turns,\n",
        "+        \"Commands\" => Icon::Terminal,\n",
        "         _ => Icon::File,\n",
    );
    DiffText {
        runs: runs(text),
        truncated: false,
        untracked: false,
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
