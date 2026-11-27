#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    Sessions,
    Conversation,
    Subagents,
    Signals,
    Turns,
    AccountsAndUsage,
    Delegation,
    Setup,
    Versions,
    Search,
}

impl Group {
    pub const ALL: [Group; 10] = [
        Group::Sessions,
        Group::Conversation,
        Group::Subagents,
        Group::Signals,
        Group::Turns,
        Group::AccountsAndUsage,
        Group::Delegation,
        Group::Setup,
        Group::Versions,
        Group::Search,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Sessions => "Sessions",
            Group::Conversation => "Conversation",
            Group::Subagents => "Subagents",
            Group::Signals => "Signals",
            Group::Turns => "Turns",
            Group::AccountsAndUsage => "Accounts and usage",
            Group::Delegation => "Delegation",
            Group::Setup => "Setup",
            Group::Versions => "Versions",
            Group::Search => "Search",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    SessionList,
    SessionStatus,
    SessionNew,
    SessionStop,
    SessionRemove,
    SessionMove,
    FolderTrust,
    ConversationRead,
    ConversationFollow,
    ConversationContext,
    SubagentList,
    SubagentStatus,
    SubagentGlance,
    SubagentOpen,
    SubagentStop,
    SignalTurnFinished,
    SignalSubagentFinished,
    SignalNeedsYou,
    TurnSend,
    TurnHold,
    TurnTakeBack,
    TurnInterrupt,
    TurnOptions,
    PromptQuestion,
    PromptPermission,
    CommandSlash,
    AccountSwitch,
    AccountLanes,
    UsageLimits,
    UsageCost,
    CatalogModels,
    DelegateTarget,
    DelegateCaller,
    SetupRules,
    SetupSkills,
    SetupHooks,
    VersionDetect,
    VersionUpdate,
    SearchText,
}

impl Capability {
    pub const ALL: [Capability; 39] = [
        Capability::SessionList,
        Capability::SessionStatus,
        Capability::SessionNew,
        Capability::SessionStop,
        Capability::SessionRemove,
        Capability::SessionMove,
        Capability::FolderTrust,
        Capability::ConversationRead,
        Capability::ConversationFollow,
        Capability::ConversationContext,
        Capability::SubagentList,
        Capability::SubagentStatus,
        Capability::SubagentGlance,
        Capability::SubagentOpen,
        Capability::SubagentStop,
        Capability::SignalTurnFinished,
        Capability::SignalSubagentFinished,
        Capability::SignalNeedsYou,
        Capability::TurnSend,
        Capability::TurnHold,
        Capability::TurnTakeBack,
        Capability::TurnInterrupt,
        Capability::TurnOptions,
        Capability::PromptQuestion,
        Capability::PromptPermission,
        Capability::CommandSlash,
        Capability::AccountSwitch,
        Capability::AccountLanes,
        Capability::UsageLimits,
        Capability::UsageCost,
        Capability::CatalogModels,
        Capability::DelegateTarget,
        Capability::DelegateCaller,
        Capability::SetupRules,
        Capability::SetupSkills,
        Capability::SetupHooks,
        Capability::VersionDetect,
        Capability::VersionUpdate,
        Capability::SearchText,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Capability::SessionList => "session.list",
            Capability::SessionStatus => "session.status",
            Capability::SessionNew => "session.new",
            Capability::SessionStop => "session.stop",
            Capability::SessionRemove => "session.remove",
            Capability::SessionMove => "session.move",
            Capability::FolderTrust => "folder.trust",
            Capability::ConversationRead => "conversation.read",
            Capability::ConversationFollow => "conversation.follow",
            Capability::ConversationContext => "conversation.context",
            Capability::SubagentList => "subagent.list",
            Capability::SubagentStatus => "subagent.status",
            Capability::SubagentGlance => "subagent.glance",
            Capability::SubagentOpen => "subagent.open",
            Capability::SubagentStop => "subagent.stop",
            Capability::SignalTurnFinished => "signal.turn_finished",
            Capability::SignalSubagentFinished => "signal.subagent_finished",
            Capability::SignalNeedsYou => "signal.needs_you",
            Capability::TurnSend => "turn.send",
            Capability::TurnHold => "turn.hold",
            Capability::TurnTakeBack => "turn.take_back",
            Capability::TurnInterrupt => "turn.interrupt",
            Capability::TurnOptions => "turn.options",
            Capability::PromptQuestion => "prompt.question",
            Capability::PromptPermission => "prompt.permission",
            Capability::CommandSlash => "command.slash",
            Capability::AccountSwitch => "account.switch",
            Capability::AccountLanes => "account.lanes",
            Capability::UsageLimits => "usage.limits",
            Capability::UsageCost => "usage.cost",
            Capability::CatalogModels => "catalog.models",
            Capability::DelegateTarget => "delegate.target",
            Capability::DelegateCaller => "delegate.caller",
            Capability::SetupRules => "setup.rules",
            Capability::SetupSkills => "setup.skills",
            Capability::SetupHooks => "setup.hooks",
            Capability::VersionDetect => "version.detect",
            Capability::VersionUpdate => "version.update",
            Capability::SearchText => "search.text",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Capability::SessionList => "Session list",
            Capability::SessionStatus => "Session status",
            Capability::SessionNew => "New session",
            Capability::SessionStop => "Stop a session",
            Capability::SessionRemove => "Remove a session",
            Capability::SessionMove => "Move a session",
            Capability::FolderTrust => "Folder trust",
            Capability::ConversationRead => "Conversation",
            Capability::ConversationFollow => "Follow a running turn",
            Capability::ConversationContext => "Context size",
            Capability::SubagentList => "Subagent list",
            Capability::SubagentStatus => "Subagent status",
            Capability::SubagentGlance => "Subagent glance",
            Capability::SubagentOpen => "Open a subagent",
            Capability::SubagentStop => "Stop a subagent",
            Capability::SignalTurnFinished => "Turn finished",
            Capability::SignalSubagentFinished => "Subagent finished",
            Capability::SignalNeedsYou => "Needs you",
            Capability::TurnSend => "Send a message",
            Capability::TurnHold => "Hold a message",
            Capability::TurnTakeBack => "Take a message back",
            Capability::TurnInterrupt => "Interrupt a turn",
            Capability::TurnOptions => "Model and effort",
            Capability::PromptQuestion => "Answer a question",
            Capability::PromptPermission => "Answer a permission",
            Capability::CommandSlash => "Slash commands",
            Capability::AccountSwitch => "Switch accounts",
            Capability::AccountLanes => "Two accounts at once",
            Capability::UsageLimits => "Plan limits",
            Capability::UsageCost => "Cost",
            Capability::CatalogModels => "Model list",
            Capability::DelegateTarget => "Run delegated jobs",
            Capability::DelegateCaller => "Delegate from a session",
            Capability::SetupRules => "Rules",
            Capability::SetupSkills => "Skills",
            Capability::SetupHooks => "Hooks",
            Capability::VersionDetect => "Installed version",
            Capability::VersionUpdate => "Update",
            Capability::SearchText => "Search text",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Capability::SessionList
            | Capability::SessionStatus
            | Capability::SessionNew
            | Capability::SessionStop
            | Capability::SessionRemove
            | Capability::SessionMove
            | Capability::FolderTrust => Group::Sessions,
            Capability::ConversationRead
            | Capability::ConversationFollow
            | Capability::ConversationContext => Group::Conversation,
            Capability::SubagentList
            | Capability::SubagentStatus
            | Capability::SubagentGlance
            | Capability::SubagentOpen
            | Capability::SubagentStop => Group::Subagents,
            Capability::SignalTurnFinished
            | Capability::SignalSubagentFinished
            | Capability::SignalNeedsYou => Group::Signals,
            Capability::TurnSend
            | Capability::TurnHold
            | Capability::TurnTakeBack
            | Capability::TurnInterrupt
            | Capability::TurnOptions
            | Capability::PromptQuestion
            | Capability::PromptPermission
            | Capability::CommandSlash => Group::Turns,
            Capability::AccountSwitch
            | Capability::AccountLanes
            | Capability::UsageLimits
            | Capability::UsageCost
            | Capability::CatalogModels => Group::AccountsAndUsage,
            Capability::DelegateTarget | Capability::DelegateCaller => Group::Delegation,
            Capability::SetupRules | Capability::SetupSkills | Capability::SetupHooks => {
                Group::Setup
            }
            Capability::VersionDetect | Capability::VersionUpdate => Group::Versions,
            Capability::SearchText => Group::Search,
        }
    }

    pub fn parse(id: &str) -> Option<Capability> {
        Capability::ALL.into_iter().find(|c| c.id() == id)
    }
}
