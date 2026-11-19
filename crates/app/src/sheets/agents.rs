use notchling_engine::install::hooks::{FENCE_EVENT, FENCE_MATCHER};
use notchling_locale::{self as t, Text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Organizations,
    Agents,
    Skills,
    Activity,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::General,
        Page::Organizations,
        Page::Agents,
        Page::Skills,
        Page::Activity,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => t::settings::page_general(),
            Page::Organizations => t::organizations::page(),
            Page::Agents => t::settings::page_agents(),
            Page::Skills => t::settings::page_skills(),
            Page::Activity => t::settings::page_activity(),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Page::General => "general",
            Page::Organizations => "organizations",
            Page::Agents => "agents",
            Page::Skills => "skills",
            Page::Activity => "activity",
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
            Tool::ClaudeCode => t::settings::tool_claude_code(),
        }
    }

    pub fn sections(self) -> &'static [Section] {
        match self {
            Tool::ClaudeCode => &[Section::Client, Section::Chrome, Section::Hooks],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Client,
    Chrome,
    Hooks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookInfo {
    pub event: &'static str,
    pub matcher: &'static str,
    pub what: Text,
}

pub const HOOKS: [HookInfo; 6] = [
    HookInfo {
        event: "PermissionRequest",
        matcher: "*",
        what: t::settings::HOOK_PERMISSION_REQUEST,
    },
    HookInfo {
        event: "PreToolUse",
        matcher: "AskUserQuestion",
        what: t::settings::HOOK_ASK_USER_QUESTION,
    },
    HookInfo {
        event: "PreToolUse",
        matcher: "Artifact",
        what: t::settings::HOOK_ARTIFACT_READ,
    },
    HookInfo {
        event: "PostToolUse",
        matcher: "Artifact",
        what: t::settings::HOOK_ARTIFACT_SAVED,
    },
    HookInfo {
        event: "PostToolUseFailure",
        matcher: "Artifact",
        what: t::settings::HOOK_ARTIFACT_READ_FAILED,
    },
    HookInfo {
        event: FENCE_EVENT,
        matcher: FENCE_MATCHER,
        what: t::settings::HOOK_FENCE,
    },
];

pub fn info(event: &str, matcher: &str) -> Option<&'static HookInfo> {
    HOOKS
        .iter()
        .find(|h| h.event == event && h.matcher == matcher)
}
