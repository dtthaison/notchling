use super::choices::{Answerer, DoneOrFailed};
use crate::field::{Count, Hex};
use crate::field_ref::{Device, Ref};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum SettingKey {
        Mode = "mode",
        Active = "active",
        KeepDisplayOn = "keepDisplayOn",
        ContinueAfterSleep = "continueAfterSleep",
        HideNotch = "hideNotch",
        AnswerFromNotch = "answerFromNotch",
        KeepPages = "keepPages",
        ShowPet = "showPet",
        ReplyFromNotch = "replyFromNotch",
        ShowLastReply = "showLastReply",
        AskedAccessibility = "askedAccessibility",
        NotchWidth = "notchWidth",
        Dismissed = "dismissed",
        HiddenAgents = "hiddenAgents",
        Accounts = "accounts",
        UntrackedCostAccount = "untrackedCostAccount",
        AntigravityAccounts = "antigravityAccounts",
        ClaudeDesktopAccounts = "claudeDesktopAccounts",
        CodexAccounts = "codexAccounts",
        LimitMode = "limitMode",
        LimitSwitchAt = "limitSwitchAt",
        CodexLimitMode = "codexLimitMode",
        AntigravityLimitMode = "antigravityLimitMode",
        CodexNetwork = "codexNetwork",
        MigratedFromAwake = "migratedFromAwake",
        ChatClosedSessions = "chatClosedSessions",
        ChatUsedSessions = "chatUsedSessions",
        ChatInspector = "chatInspector",
        CodexPicks = "codexPicks",
        ChatProjectFolders = "chatProjectFolders",
        NewSessionChoice = "newSessionChoice",
        ChatOpen = "chatOpen",
        ChatSelection = "chatSelection",
        ChatWindowFrame = "chatWindowFrame",
        ChatPanels = "chatPanels",
        ChatPageWidth = "chatPageWidth",
        ChatHistory = "chatHistory",
        Appearance = "appearance",
        SearchShortcut = "searchShortcut",
        VaultReminderSent = "vaultReminderSent",
        BridgeEnabled = "bridgeEnabled",
        DebugAccessCertsFile = "debugAccessCertsFile",
        Unknown = "unknown",
    }
}

log_choice! {
    pub enum SettingValue {
        Off = "off",
        On = "on",
        WhileWorking = "whileWorking",
        Small = "small",
        Medium = "medium",
        Large = "large",
        Notify = "notify",
        NotifyAndSwitch = "notifyAndSwitch",
        Mb2 = "2mb",
        Mb4 = "4mb",
        Mb8 = "8mb",
        Mb16 = "16mb",
        System = "system",
        Light = "light",
        Dark = "dark",
    }
}

log_choice! {
    pub enum SettingClass {
        Free = "free",
        Ask = "ask",
        Never = "never",
    }
}

log_choice! {
    pub enum AgentSetResult {
        Set = "set",
        Asked = "asked",
        Allowed = "allowed",
        Denied = "denied",
        Expired = "expired",
        Refused = "refused",
        UnknownKey = "unknown_key",
        BadValue = "bad_value",
        Failed = "failed",
        NoApp = "no_app",
        Full = "full",
    }
}

log_choice! {
    pub enum HookTarget {
        Claude = "claude",
        Codex = "codex",
        Agy = "agy",
    }
}

log_choice! {
    pub enum InstallAction {
        Installed = "installed",
        Removed = "removed",
        Repaired = "repaired",
    }
}

kinds! {
    group: Group::Settings,
    SettingsAgent = "settings.agent", kv 1 {
        title: "Settings Changed by Agents",
        records: "An agent tried to change a setting: which one, its class, how the value was written, and how the request ended.",
        why: "Shows every setting an agent changed or asked to change, and whether you or your phone allowed it.",
        default: On,
        lock: Lock::Always("Changes agents make to settings are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 0,
        fields {
            key: SettingKey,
            key_hash: Option<Hex>,
            class: Option<SettingClass>,
            value_flag: Option<bool>,
            value_number: Option<Count>,
            value_choice: Option<SettingValue>,
            value_chars: Option<Count>,
            value_hash: Option<Hex>,
            result: AgentSetResult,
            answered_by: Option<Answerer>,
            device: Option<Ref<Device>>,
        }
    }
    SettingsChanged = "settings.changed", kv 1 {
        title: "Your Settings Changes",
        records: "A setting was changed in the app: which one and how the new value was written.",
        why: "Lets you see what changed and when if the app starts behaving differently.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::App],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            key: SettingKey,
            key_hash: Option<Hex>,
            value_flag: Option<bool>,
            value_number: Option<Count>,
            value_choice: Option<SettingValue>,
            value_chars: Option<Count>,
            value_hash: Option<Hex>,
        }
    }
    InstallHook = "install.hook", kv 1 {
        title: "Hook Installed or Removed",
        records: "Notchling's hook was installed, removed or repaired for Claude Code, Codex or Antigravity.",
        why: "The hook decides what Notchling sees of an agent, so every change to it is recorded.",
        default: On,
        lock: Lock::Always("Changes to the hook are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            target: HookTarget,
            action: InstallAction,
            result: DoneOrFailed,
        }
    }
}
