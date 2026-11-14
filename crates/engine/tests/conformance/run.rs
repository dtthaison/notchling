use std::path::PathBuf;

use notchling_engine::contract::conformance::{check, Checked};
use notchling_engine::contract::{Capability, Kind, Registry, Tool};

pub fn root(tool: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conformance")
        .join(tool)
}

pub fn cell(tool: Tool, c: Capability, root: PathBuf) {
    let registry = Registry::standard();
    let support = registry.support(tool, c);
    let entry = format!("{} {}", tool.name(), c.id());
    match support.kind() {
        Kind::Native | Kind::Fallback => {
            if registry.is_wired(tool, c) {
                match check(&registry, tool, c, &root) {
                    Checked::Passed => {}
                    Checked::NotWritten => {
                        panic!("{entry} is wired but has no conformance check")
                    }
                }
            } else {
                println!("skipped (unwired): {entry}");
            }
        }
        Kind::NotApplicable | Kind::Gap => {
            assert_eq!(registry.of(tool).support(c), support, "{entry}");
            assert!(!support.line().is_empty(), "{entry}");
        }
    }
}

pub fn summary() -> String {
    let (mut wired, mut unwired, mut not_applicable, mut gaps) = (0, 0, 0, 0);
    for c in Registry::standard().cells() {
        match c.support.kind() {
            Kind::Native | Kind::Fallback if c.wired => wired += 1,
            Kind::Native | Kind::Fallback => unwired += 1,
            Kind::NotApplicable => not_applicable += 1,
            Kind::Gap => gaps += 1,
        }
    }
    format!(
        "conformance: wired {wired}, unwired {unwired}, not applicable {not_applicable}, gaps {gaps}"
    )
}

macro_rules! each_capability {
    ($emit:ident, $tool:expr, $name:literal) => {
        $emit!(session_list, SessionList, $tool, $name);
        $emit!(session_status, SessionStatus, $tool, $name);
        $emit!(session_new, SessionNew, $tool, $name);
        $emit!(session_stop, SessionStop, $tool, $name);
        $emit!(session_remove, SessionRemove, $tool, $name);
        $emit!(session_move, SessionMove, $tool, $name);
        $emit!(folder_trust, FolderTrust, $tool, $name);
        $emit!(conversation_read, ConversationRead, $tool, $name);
        $emit!(conversation_follow, ConversationFollow, $tool, $name);
        $emit!(conversation_context, ConversationContext, $tool, $name);
        $emit!(subagent_list, SubagentList, $tool, $name);
        $emit!(subagent_status, SubagentStatus, $tool, $name);
        $emit!(subagent_glance, SubagentGlance, $tool, $name);
        $emit!(subagent_open, SubagentOpen, $tool, $name);
        $emit!(subagent_stop, SubagentStop, $tool, $name);
        $emit!(signal_turn_finished, SignalTurnFinished, $tool, $name);
        $emit!(
            signal_subagent_finished,
            SignalSubagentFinished,
            $tool,
            $name
        );
        $emit!(signal_needs_you, SignalNeedsYou, $tool, $name);
        $emit!(turn_send, TurnSend, $tool, $name);
        $emit!(turn_hold, TurnHold, $tool, $name);
        $emit!(turn_take_back, TurnTakeBack, $tool, $name);
        $emit!(turn_interrupt, TurnInterrupt, $tool, $name);
        $emit!(turn_options, TurnOptions, $tool, $name);
        $emit!(prompt_question, PromptQuestion, $tool, $name);
        $emit!(prompt_permission, PromptPermission, $tool, $name);
        $emit!(command_slash, CommandSlash, $tool, $name);
        $emit!(account_switch, AccountSwitch, $tool, $name);
        $emit!(account_lanes, AccountLanes, $tool, $name);
        $emit!(usage_limits, UsageLimits, $tool, $name);
        $emit!(usage_cost, UsageCost, $tool, $name);
        $emit!(catalog_models, CatalogModels, $tool, $name);
        $emit!(delegate_target, DelegateTarget, $tool, $name);
        $emit!(delegate_caller, DelegateCaller, $tool, $name);
        $emit!(setup_rules, SetupRules, $tool, $name);
        $emit!(setup_skills, SetupSkills, $tool, $name);
        $emit!(setup_hooks, SetupHooks, $tool, $name);
        $emit!(version_detect, VersionDetect, $tool, $name);
        $emit!(version_update, VersionUpdate, $tool, $name);
        $emit!(search_text, SearchText, $tool, $name);
    };
}
