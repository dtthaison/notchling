#[cfg(target_os = "macos")]
use crate::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolMark {
    Claude,
    Codex,
    Antigravity,
}

impl ToolMark {
    pub fn name(self) -> &'static str {
        match self {
            ToolMark::Claude => "Claude Code",
            ToolMark::Codex => "Codex",
            ToolMark::Antigravity => "Antigravity CLI",
        }
    }

    pub fn bundle_ids(self) -> &'static [&'static str] {
        match self {
            ToolMark::Claude => &["com.anthropic.claudefordesktop"],
            ToolMark::Codex => &["com.openai.codex"],
            ToolMark::Antigravity => &["com.google.antigravity", "com.google.antigravity-ide"],
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn icon(self) -> Icon {
        match self {
            ToolMark::Claude => Icon::Claude,
            ToolMark::Codex => Icon::Code,
            ToolMark::Antigravity => Icon::Paperplane,
        }
    }
}
