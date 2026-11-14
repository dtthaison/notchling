use super::{Tool, Version};

const READ_CHARS: usize = 200;

pub fn cli_output(tool: Tool, text: &str) -> Option<Version> {
    let first = text.split(['\n', '\r']).next().unwrap_or("");
    let line: String = first.chars().take(READ_CHARS).collect();
    let line = line.trim_end_matches(' ');
    let number = match tool {
        Tool::Claude => line.strip_suffix(" (Claude Code)")?,
        Tool::Codex => line.strip_prefix("codex-cli ")?,
        Tool::Agy => line.strip_prefix("agy ")?,
        Tool::Notchling => line.strip_prefix("notchling ")?,
    };
    Version::parse(number)
}
