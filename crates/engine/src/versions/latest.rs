use super::{Tool, Version};

const MAX_BODY: usize = 1024 * 1024;

pub const CLAUDE_LATEST: &str = "https://downloads.claude.ai/claude-code-releases/latest";
pub const CODEX_LATEST: &str = "https://registry.npmjs.org/@openai/codex/latest";
pub const AGY_MANIFESTS: &str =
    "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";

pub fn agy_platform(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("darwin_arm64"),
        ("macos", "x86_64") => Some("darwin_amd64"),
        ("linux", "aarch64") => Some("linux_arm64"),
        ("linux", "x86_64") => Some("linux_amd64"),
        _ => None,
    }
}

pub fn source_for(tool: Tool, os: &str, arch: &str) -> Option<String> {
    match tool {
        Tool::Claude => Some(CLAUDE_LATEST.to_string()),
        Tool::Codex => Some(CODEX_LATEST.to_string()),
        Tool::Agy => agy_platform(os, arch).map(|p| format!("{AGY_MANIFESTS}/{p}.json")),
        Tool::Notchling => None,
    }
}

pub fn source(tool: Tool) -> Option<String> {
    source_for(tool, std::env::consts::OS, std::env::consts::ARCH)
}

pub fn host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then_some(host)
}

pub fn parse(tool: Tool, body: &str) -> Option<Version> {
    if body.len() > MAX_BODY {
        return None;
    }
    match tool {
        Tool::Claude => {
            let line = body.lines().next()?.trim();
            Version::parse(line)
        }
        Tool::Codex | Tool::Agy => {
            let json: serde_json::Value = serde_json::from_str(body).ok()?;
            Version::parse(json.get("version")?.as_str()?)
        }
        Tool::Notchling => None,
    }
}
