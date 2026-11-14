use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Codex,
    #[serde(alias = "agy")]
    Antigravity,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Antigravity];

    pub fn parse(name: &str) -> Option<Tool> {
        match name {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            "antigravity" | "agy" => Some(Tool::Antigravity),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Antigravity => "antigravity",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Antigravity => "Antigravity",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    pub fn of_row(row_id: &str) -> Tool {
        if row_id.starts_with("codex:") || row_id.starts_with("codex-new:") {
            Tool::Codex
        } else if row_id.starts_with("antigravity:")
            || row_id.starts_with("antigravity-new:")
            || row_id.starts_with("agy:")
            || row_id.starts_with("agy-new:")
        {
            Tool::Antigravity
        } else {
            Tool::Claude
        }
    }

    pub fn row_prefix(self) -> &'static str {
        match self {
            Tool::Claude => "",
            Tool::Codex => "codex:",
            Tool::Antigravity => "antigravity:",
        }
    }
}
