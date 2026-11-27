use serde::{Deserialize, Serialize};

use super::query::filter::Filter;

pub const UNKNOWN_ACCOUNT: &str = "unknown";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionTool {
    Claude,
    Codex,
    Antigravity,
}

impl SessionTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "agy" => Some(Self::Antigravity),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub key: String,
    pub tool_key: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    pub folder: String,
    pub path: String,
    pub config_dir: Option<String>,
    pub modified: f64,
    #[serde(default)]
    pub size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<f64>,
}

impl SessionMeta {
    pub fn tool(&self) -> SessionTool {
        SessionTool::from_key(&self.tool_key).unwrap_or(SessionTool::Claude)
    }

    pub fn facets(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![("tool", self.tool_key.clone())];
        if !self.folder.is_empty() {
            out.push(("project", self.folder.clone()));
        }
        out.push((
            "account",
            self.account
                .clone()
                .unwrap_or_else(|| UNKNOWN_ACCOUNT.into()),
        ));
        out
    }

    pub fn matches(&self, filter: &Filter) -> bool {
        let facets = self.facets();
        let pairs: Vec<(&str, &str)> = facets.iter().map(|(n, v)| (*n, v.as_str())).collect();
        filter.matches(&pairs, Some(self.modified))
    }
}

pub fn codex_id(file_name: &str) -> String {
    let chars: Vec<char> = file_name.chars().collect();
    let end = chars.len().saturating_sub(".jsonl".len());
    let start = end.saturating_sub(36);
    chars[start..end].iter().collect()
}

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
