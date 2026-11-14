use serde::{Deserialize, Serialize};

use super::Tool;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        [Some(path), link]
            .into_iter()
            .flatten()
            .map(|p| of_one(tool, p))
            .find(|m| *m != Method::Unknown)
            .unwrap_or(Method::Unknown)
    }
}

fn of_one(tool: Tool, path: &str) -> Method {
    if path.contains("Caskroom") || path.contains("Cellar") {
        return Method::Brew;
    }
    if path.contains("node_modules") {
        return Method::Npm;
    }
    match tool {
        Tool::Claude if path.contains(".local/share/claude/versions/") => Method::Native,
        Tool::Codex if path.contains(".codex/packages/standalone") => Method::Standalone,
        Tool::Agy if path.contains(".local/bin/agy") => Method::Standalone,
        Tool::Notchling if path.contains(".local/share/notchling/versions/") => Method::Release,
        _ => Method::Unknown,
    }
}

pub fn can_pin(tool: Tool, method: Method) -> bool {
    match (tool, method) {
        (Tool::Claude, Method::Native | Method::Npm) => true,
        (Tool::Codex, Method::Standalone | Method::Npm) => true,
        (Tool::Notchling, Method::Release) => true,
        _ => false,
    }
}
