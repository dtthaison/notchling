use super::Tool;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Native,
    Standalone,
    Npm,
    Brew,
    Apt,
    Release,
    Unknown,
}

impl Method {
    pub fn title(self) -> &'static str {
        match self {
            Method::Native => "native installer",
            Method::Standalone => "standalone installer",
            Method::Npm => "npm",
            Method::Brew => "Homebrew",
            Method::Apt => "apt",
            Method::Release => "release",
            Method::Unknown => "unknown",
        }
    }

    pub fn from_path(tool: Tool, path: &str, link: Option<&str>) -> Method {
        link.map(|l| classify(tool, l))
            .filter(|m| *m != Method::Unknown)
            .unwrap_or_else(|| classify(tool, path))
    }
}

fn classify(tool: Tool, text: &str) -> Method {
    if text.contains("node_modules") {
        return Method::Npm;
    }
    if text.contains("Caskroom") || text.contains("Cellar") {
        return Method::Brew;
    }
    match tool {
        Tool::Claude if text.contains(".local/share/claude/versions/") => Method::Native,
        Tool::Codex if text.contains(".codex/packages/standalone") => Method::Standalone,
        Tool::Agy if text.ends_with(".local/bin/agy") => Method::Standalone,
        Tool::Notchling if text.contains(".local/share/notchling/versions/") => Method::Release,
        _ => Method::Unknown,
    }
}

pub fn can_pin(tool: Tool, method: Method) -> bool {
    matches!(
        (tool, method),
        (Tool::Claude, Method::Native | Method::Npm)
            | (Tool::Codex, Method::Standalone | Method::Npm)
            | (Tool::Notchling, Method::Release)
    )
}
