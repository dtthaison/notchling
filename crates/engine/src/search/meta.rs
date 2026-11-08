//! Which tool a session belongs to, the index's per-session record, and small pure helpers.

use serde::{Deserialize, Serialize};

/// The tool whose conversation a session is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionTool {
    Claude,
    Codex,
    Antigravity,
}

impl SessionTool {
    /// The key stored in the index and its cache file.
    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
        }
    }

    /// The tool for a stored key; `None` for an unknown one.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "agy" => Some(Self::Antigravity),
            _ => None,
        }
    }
}

/// One conversation in the index: what search shows and what Chat needs to open it. The JSON keys
/// match the Swift app's `search-index.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    /// `<tool>:<session id>`, unique across tools.
    pub key: String,
    pub tool_key: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    pub folder: String,
    /// The transcript or rollout file; empty for Antigravity, whose steps aren't read.
    pub path: String,
    /// The Claude Code config folder it was found in when that's a lane; `None` for ~/.claude.
    pub config_dir: Option<String>,
    /// When the conversation last changed (seconds since 1970).
    pub modified: f64,
    /// The file's size when it was indexed; with `modified`, tells whether it needs reading again.
    #[serde(default)]
    pub size: i64,
}

impl SessionMeta {
    /// The tool named by `tool_key`; Claude Code for an unknown key, like the Swift app.
    pub fn tool(&self) -> SessionTool {
        SessionTool::from_key(&self.tool_key).unwrap_or(SessionTool::Claude)
    }
}

/// The session id at the end of `rollout-<time>-<id>.jsonl` (a 36-character UUID).
pub fn codex_id(file_name: &str) -> String {
    let chars: Vec<char> = file_name.chars().collect();
    let end = chars.len().saturating_sub(".jsonl".len());
    let start = end.saturating_sub(36);
    chars[start..end].iter().collect()
}

/// How long ago, in a word or two: "now", "5m", "3h", "2d", "3w". Times are seconds since 1970.
pub fn age_text(from: f64, now: f64) -> String {
    let s = (now - from).max(0.0);
    if s < 60.0 {
        "now".to_string()
    } else if s < 3600.0 {
        format!("{}m", (s / 60.0) as i64)
    } else if s < 86400.0 {
        format!("{}h", (s / 3600.0) as i64)
    } else if s < 86400.0 * 14.0 {
        format!("{}d", (s / 86400.0) as i64)
    } else {
        format!("{}w", (s / (86400.0 * 7.0)) as i64)
    }
}
