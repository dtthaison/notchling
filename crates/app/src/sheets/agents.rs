use notchling_engine::install::hooks::{
    FENCE_EVENT, FENCE_MATCHER, GUARD_START_EVENT, GUARD_STOP_EVENT, GUARD_TOOLS,
};
use notchling_locale::{self as t, Text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Organizations,
    Agents,
    Skills,
    Costs,
    Activity,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page::General,
        Page::Organizations,
        Page::Agents,
        Page::Skills,
        Page::Costs,
        Page::Activity,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => t::settings::page_general(),
            Page::Organizations => t::organizations::page(),
            Page::Agents => t::settings::page_agents(),
            Page::Skills => t::settings::page_skills(),
            Page::Costs => t::settings::page_costs(),
            Page::Activity => t::settings::page_activity(),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Page::General => "general",
            Page::Organizations => "organizations",
            Page::Agents => "agents",
            Page::Skills => "skills",
            Page::Costs => "costs",
            Page::Activity => "activity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    ClaudeCode,
    Codex,
    Antigravity,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::ClaudeCode, Tool::Codex, Tool::Antigravity];

    pub fn title(self) -> &'static str {
        match self {
            Tool::ClaudeCode => t::settings::tool_claude_code(),
            Tool::Codex => t::settings::tool_codex(),
            Tool::Antigravity => t::settings::tool_antigravity(),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude-code",
            Tool::Codex => "codex",
            Tool::Antigravity => "antigravity",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn contract(self) -> notchling_engine::contract::Tool {
        match self {
            Tool::ClaudeCode => notchling_engine::contract::Tool::Claude,
            Tool::Codex => notchling_engine::contract::Tool::Codex,
            Tool::Antigravity => notchling_engine::contract::Tool::Antigravity,
        }
    }

    pub fn sections(self) -> &'static [Section] {
        match self {
            Tool::ClaudeCode => &[
                Section::Settings,
                Section::Client,
                Section::Chrome,
                Section::Hooks,
                Section::Capabilities,
            ],
            Tool::Codex | Tool::Antigravity => &[Section::Settings, Section::Capabilities],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Settings,
    Client,
    Chrome,
    Hooks,
    Capabilities,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookInfo {
    pub event: &'static str,
    pub matcher: &'static str,
    pub what: Text,
}

pub const HOOKS: [HookInfo; 9] = [
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
    HookInfo {
        event: FENCE_EVENT,
        matcher: GUARD_TOOLS,
        what: t::settings::HOOK_GUARD_TOOLS,
    },
    HookInfo {
        event: GUARD_START_EVENT,
        matcher: FENCE_MATCHER,
        what: t::settings::HOOK_GUARD_START,
    },
    HookInfo {
        event: GUARD_STOP_EVENT,
        matcher: FENCE_MATCHER,
        what: t::settings::HOOK_GUARD_STOP,
    },
];

pub fn info(event: &str, matcher: &str) -> Option<&'static HookInfo> {
    HOOKS
        .iter()
        .find(|h| h.event == event && h.matcher == matcher)
}
