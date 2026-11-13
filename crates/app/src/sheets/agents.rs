#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Agents,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::General, Page::Agents];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Agents => "Agents",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    ClaudeCode,
}

impl Tool {
    pub const ALL: [Tool; 1] = [Tool::ClaudeCode];

    pub fn title(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "Claude Code",
        }
    }

    pub fn sections(self) -> &'static [Section] {
        match self {
            Tool::ClaudeCode => &[Section::Hooks],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Hooks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookInfo {
    pub event: &'static str,
    pub matcher: &'static str,
    pub what: &'static str,
}

pub const HOOKS: [HookInfo; 5] = [
    HookInfo {
        event: "PermissionRequest",
        matcher: "*",
        what: "Claude's permission asks show in the notch, in Chat and on the phone, not only in the terminal.",
    },
    HookInfo {
        event: "PreToolUse",
        matcher: "AskUserQuestion",
        what: "Claude's multiple-choice questions can be answered from the notch, Chat or the phone.",
    },
    HookInfo {
        event: "PreToolUse",
        matcher: "Artifact",
        what: "When Claude reads a page saved on this Mac, it is given the local copy's path instead of the live page.",
    },
    HookInfo {
        event: "PostToolUse",
        matcher: "Artifact",
        what: "After a publish, a local copy is saved for the Pages window and notchling pages.",
    },
    HookInfo {
        event: "PostToolUseFailure",
        matcher: "Artifact",
        what: "When a page read fails (a 404 or another account's page), Claude gets the local copy's path.",
    },
];

pub const PASS_NOTE: &str = "The permission and question hooks pass straight through, and Claude asks in its terminal, when the daemon, the app or Act on Sessions is off, and in a claude -p run. The Artifact hooks work without them.";

pub fn info(event: &str, matcher: &str) -> Option<&'static HookInfo> {
    HOOKS
        .iter()
        .find(|h| h.event == event && h.matcher == matcher)
}
