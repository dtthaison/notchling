#![deny(clippy::wildcard_enum_match_arm)]

use crate::contract::{Capability, Flow, Support};

pub fn support(c: Capability) -> Support {
    match c {
        Capability::SessionList => Support::native("Rollouts under CODEX_HOME/sessions"),
        Capability::SessionStatus => {
            Support::native("Rollout tail (turn_open) and Notchling's own runs")
        }
        Capability::SessionNew => Support::native("First codex exec turn, shown as a draft row"),
        Capability::SessionStop => {
            Support::not_applicable("Nothing runs between Codex turns; Stop interrupts the turn")
        }
        Capability::SessionRemove => Support::native("codex delete"),
        Capability::SessionMove => Support::gap("phase 5: a lane needs its own CODEX_HOME"),
        Capability::FolderTrust => Support::not_applicable(
            "codex exec asks no trust question; the sandbox is set per turn",
        ),
        Capability::ConversationRead => Support::native("The rollout reader"),
        Capability::ConversationFollow => {
            Support::native("File watch on the rollout, --json for its own runs")
        }
        Capability::ConversationContext => Support::native("model_context_window in the rollout"),
        Capability::SubagentList => Support::gap("phase 1: child rollouts by parent_thread_id"),
        Capability::SubagentStatus => {
            Support::gap("phase 1: the child's task_complete or turn_aborted")
        }
        Capability::SubagentGlance => Support::gap_meanwhile(
            "phase 1: the child's token_count and newest call",
            Flow::DelegateJobs,
        ),
        Capability::SubagentOpen => Support::gap("phase 1: the rollout from the child's own turn"),
        Capability::SubagentStop => {
            Support::not_applicable("Only the model's interrupt_agent stops a Codex subagent")
        }
        Capability::SignalTurnFinished => {
            Support::native("task_complete and turn_aborted in the rollout")
        }
        Capability::SignalSubagentFinished => Support::gap("phase 2: the child's task_complete"),
        Capability::SignalNeedsYou => Support::not_applicable("codex exec never asks"),
        Capability::TurnSend => Support::native("codex exec resume"),
        Capability::TurnHold => Support::fallback(
            Flow::HeldQueue,
            "codex exec takes one message per turn; Notchling holds the rest",
        ),
        Capability::TurnTakeBack => Support::gap("phase 4: held-queue"),
        Capability::TurnInterrupt => Support::native("SIGINT"),
        Capability::TurnOptions => Support::native("Model and effort per turn"),
        Capability::PromptQuestion => Support::not_applicable("codex exec can't ask a question"),
        Capability::PromptPermission => {
            Support::not_applicable("Codex turns run in a sandbox and never ask")
        }
        Capability::CommandSlash => Support::gap("phase 5: Codex's slash commands in Chat"),
        Capability::AccountSwitch => Support::native("Saved logins in auth.json"),
        Capability::AccountLanes => Support::gap("phase 5: CODEX_HOME per account"),
        Capability::UsageLimits => Support::native("Plan limits from the provider's usage source"),
        Capability::UsageCost => Support::native("Rollout token counts priced from the catalog"),
        Capability::CatalogModels => Support::native("models_cache.json"),
        Capability::DelegateTarget => Support::native("codex exec"),
        Capability::DelegateCaller => Support::gap("phase 3: CODEX_THREAD_ID"),
        Capability::SetupRules => Support::gap("design: 0019"),
        Capability::SetupSkills => Support::native("install::skills writes the notchling-* skills"),
        Capability::SetupHooks => Support::gap("design: 0019 (planned not applicable)"),
        Capability::VersionDetect => Support::gap("design: version-sync"),
        Capability::VersionUpdate => Support::gap("design: version-sync"),
        Capability::SearchText => Support::native("Rollout text in Search's sessions corpus"),
    }
}
