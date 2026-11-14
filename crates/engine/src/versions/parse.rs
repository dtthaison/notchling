use super::{Tool, Version};

const READ_CHARS: usize = 200;

pub fn cli_output(tool: Tool, text: &str) -> Option<Version> {
    let head: String = text.chars().take(READ_CHARS).collect();
    let line = head.split('\n').next()?.trim_end_matches('\r');
    if line
        .chars()
        .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        return None;
    }
    let word = match tool {
        Tool::Claude => line.strip_suffix(" (Claude Code)").unwrap_or(line),
        Tool::Codex => line.strip_prefix("codex-cli ")?,
        Tool::Agy => line.strip_prefix("agy ")?,
        Tool::Notchling => line.strip_prefix("notchling ")?,
    };
    Version::parse(word)
}
