#![deny(clippy::wildcard_enum_match_arm)]

use crate::contract::{Capability, Flow, Support};

pub fn support(c: Capability) -> Support {
    match c {
        Capability::SessionList => Support::native("The summaries copy, nesting_depth 0"),
        Capability::SessionStatus => Support::native("Its agy acp, else the holder's log"),
        Capability::SessionNew => Support::native("ACP session/new, shown as a draft row"),
        Capability::SessionStop => Support::native("Closes its agy acp"),
        Capability::SessionRemove => Support::gap("phase 5: hide-row; agy has no delete"),
        Capability::SessionMove => {
            Support::not_applicable("One Antigravity login per Mac; a switch moves every session")
        }
        Capability::FolderTrust => Support::native("trustedWorkspaces through agy_trust"),
        Capability::ConversationRead => Support::native("ACP session/load, else the log"),
        Capability::ConversationFollow => Support::native("ACP updates, else the log"),
        Capability::ConversationContext => {
            Support::native("Tokens in the log; the window is the catalog's")
        }
        Capability::SubagentList => {
            Support::native("Summaries rows whose parent_conversation_id is the session")
        }
        Capability::SubagentStatus => {
            Support::native("Summaries status and killed, send_message, the parent's [Message]")
        }
        Capability::SubagentGlance => Support::native("The child log's tokens and newest step"),
        Capability::SubagentOpen => Support::native("The child's log"),
        Capability::SubagentStop => Support::not_applicable(
            "The Agent Manager's kill is inside agy; Notchling can't reach it",
        ),
        Capability::SignalTurnFinished => {
            Support::native("ACP stopReason; log PLANNER_RESPONSE DONE")
        }
        Capability::SignalSubagentFinished => {
            Support::gap("phase 2: the child goes idle after send_message")
        }
        Capability::SignalNeedsYou => Support::native("ACP request_permission"),
        Capability::TurnSend => Support::native("ACP session/prompt"),
        Capability::TurnHold => Support::gap("phase 4: held-queue; the box is off while it works"),
        Capability::TurnTakeBack => Support::gap("phase 4: held-queue"),
        Capability::TurnInterrupt => Support::native("ACP session/cancel"),
        Capability::TurnOptions => Support::native("ACP session/set_config_option"),
        Capability::PromptQuestion => {
            Support::not_applicable("ACP carries permission requests only")
        }
        Capability::PromptPermission => Support::native("ACP request_permission"),
        Capability::CommandSlash => Support::gap("phase 5: available_commands_update in Chat"),
        Capability::AccountSwitch => Support::native("The Keychain item and its mirror file"),
        Capability::AccountLanes => Support::not_applicable("One Antigravity login per Mac"),
        Capability::UsageLimits => Support::native("The local language server"),
        Capability::UsageCost => Support::native("Log token counts priced from the catalog"),
        Capability::CatalogModels => Support::gap("phase 5: context windows in catalog.json"),
        Capability::DelegateTarget => Support::native("agy acp"),
        Capability::DelegateCaller => {
            Support::gap("phase 3: NOTCHLING_SESSION, then the process tree")
        }
        Capability::SetupRules => Support::gap("design: 0019"),
        Capability::SetupSkills => Support::native("install::skills writes the notchling-* skills"),
        Capability::SetupHooks => Support::gap("design: 0019 (planned not applicable)"),
        Capability::VersionDetect => Support::gap("design: version-sync"),
        Capability::VersionUpdate => Support::gap("design: version-sync"),
        Capability::SearchText => {
            Support::gap_meanwhile("phase 5: text from its logs", Flow::TitleIndex)
        }
    }
}
