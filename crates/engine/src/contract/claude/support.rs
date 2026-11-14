#![deny(clippy::wildcard_enum_match_arm)]

use crate::contract::{Capability, Flow, Support};

pub fn support(c: Capability) -> Support {
    match c {
        Capability::SessionList => {
            Support::native("jobs/ and sessions/ in every lane's config folder")
        }
        Capability::SessionStatus => Support::native("Job state, transcript and screen"),
        Capability::SessionNew => Support::native("A claude job"),
        Capability::SessionStop => Support::native("claude stop"),
        Capability::SessionRemove => Support::native("claude rm"),
        Capability::SessionMove => Support::native("Lanes: Move and Move All"),
        Capability::FolderTrust => Support::native("Trust Folder"),
        Capability::ConversationRead => Support::native("The session's transcript"),
        Capability::ConversationFollow => Support::native("File watch on the transcript"),
        Capability::ConversationContext => {
            Support::native("Transcript usage; the window comes from the catalog")
        }
        Capability::SubagentList => Support::native("subagents/agent-*.jsonl and their .meta.json"),
        Capability::SubagentStatus => {
            Support::native("task-notification, finished, handed back, 10 min quiet")
        }
        Capability::SubagentGlance => {
            Support::native("glance::peek over the subagent's transcript tail")
        }
        Capability::SubagentOpen => Support::native("The subagent's transcript"),
        Capability::SubagentStop => Support::not_applicable(
            "No outside stop for one subagent; Stop interrupts the whole turn",
        ),
        Capability::SignalTurnFinished => {
            Support::native("Job state goes from Working to Completed")
        }
        Capability::SignalSubagentFinished => {
            Support::native("task-notification in the parent transcript")
        }
        Capability::SignalNeedsYou => Support::native("Hook, transcript and screen"),
        Capability::TurnSend => Support::native("Typed through claude attach"),
        Capability::TurnHold => Support::fallback(
            Flow::HeldQueue,
            "Claude Code's own queue is lost on restart and injected mid-turn",
        ),
        Capability::TurnTakeBack => Support::fallback(
            Flow::HeldQueue,
            "The held message is Notchling's, so Notchling edits it",
        ),
        Capability::TurnInterrupt => Support::native("Esc"),
        Capability::TurnOptions => Support::native("/model and /effort"),
        Capability::PromptQuestion => Support::native("AskUserQuestion hook"),
        Capability::PromptPermission => Support::native("PermissionRequest hook"),
        Capability::CommandSlash => Support::native("slash.rs lists and runs them"),
        Capability::AccountSwitch => {
            Support::native("Saved logins in the Keychain and the config folder")
        }
        Capability::AccountLanes => Support::native("CLAUDE_CONFIG_DIR per account"),
        Capability::UsageLimits => Support::native("Plan limits from the provider's usage source"),
        Capability::UsageCost => Support::native("Transcript usage priced from the catalog"),
        Capability::CatalogModels => Support::native("Anthropic's list in data/catalog.json"),
        Capability::DelegateTarget => Support::gap("phase 3: notchling delegate claude"),
        Capability::DelegateCaller => Support::native("CLAUDE_CODE_SESSION_ID"),
        Capability::SetupRules => Support::gap("design: 0019"),
        Capability::SetupSkills => Support::native("install::skills writes the notchling-* skills"),
        Capability::SetupHooks => Support::native("install::hooks writes Notchling's five hooks"),
        Capability::VersionDetect => Support::gap("design: version-sync"),
        Capability::VersionUpdate => Support::gap("design: version-sync"),
        Capability::SearchText => Support::native("Transcript text in Search's sessions corpus"),
    }
}
