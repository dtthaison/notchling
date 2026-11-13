use std::sync::Arc;

use notchling_kit::Ink;
use serde_json::{json, Value};

use super::inspector::counted_title;
use crate::acting::Request;

pub const STATUS_OP: &str = "git.status";
pub const DIFF_OP: &str = "git.diff";

pub const EVERY_MS: i64 = 5_000;
pub const EVENT_FLOOR_MS: i64 = 1_000;
pub const SHOWN_FILES: usize = 40;

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
    pub tracking: Option<String>,
    pub files: Vec<GitFile>,
    pub truncated: bool,
}

impl GitModel {
    pub fn title(&self) -> String {
        counted_title("Git", 0, self.files.len())
    }

    pub fn hidden(&self) -> usize {
        self.files.len().saturating_sub(SHOWN_FILES)
    }

    pub fn hidden_text(&self) -> Option<String> {
        let hidden = self.hidden();
        let plus = if self.truncated { "+" } else { "" };
        match (hidden, self.truncated) {
            (0, false) => None,
            (0, true) => Some("More files not shown".to_owned()),
            _ => Some(format!("{hidden}{plus} more files not shown")),
        }
    }
}

pub fn tracking(upstream: &str, ahead: u64, behind: u64) -> String {
    match (ahead, behind) {
        (0, 0) => format!("{upstream} \u{b7} up to date"),
        (a, 0) => format!("{upstream} \u{b7} {a} ahead"),
        (0, b) => format!("{upstream} \u{b7} {b} behind"),
        (a, b) => format!("{upstream} \u{b7} {a} ahead, {b} behind"),
    }
}

pub fn parse_status(reply: &Value) -> Option<GitModel> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let branch = match (reply["detached"].as_bool(), reply["branch"].as_str()) {
        (Some(true), _) => match reply["head"].as_str() {
            Some(head) => format!("Detached at {head}"),
            None => "Detached HEAD".to_owned(),
        },
        (_, Some(name)) => name.to_owned(),
        _ => "No branch".to_owned(),
    };
    let tracking = reply["upstream"].as_str().map(|upstream| {
        tracking(
            upstream,
            reply["ahead"].as_u64().unwrap_or(0),
            reply["behind"].as_u64().unwrap_or(0),
        )
    });
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
        tracking,
        files,
        truncated: reply["truncated"].as_bool().unwrap_or(false),
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

    pub fn inflight(&self) -> bool {
        self.inflight
    }
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
        tracking: Some(tracking("origin/agent-run-oss", 2, 0)),
        files: vec![
            file("crates/app/src/chat/inspector.rs", "M", Some(14), Some(9)),
            file("crates/app/src/chat/git.rs", "A", Some(212), Some(0)),
            file("crates/engine/src/old_helper.rs", "D", Some(0), Some(48)),
            file("docs/local-api-ops.md", "M", Some(31), Some(2)),
            file("notes.md", "?", None, None),
        ],
        truncated: false,
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
