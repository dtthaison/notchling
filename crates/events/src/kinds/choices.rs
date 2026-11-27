log_choice! {
    pub enum Answerer {
        Mac = "mac",
        Phone = "phone",
        NoOne = "none",
        Main = "main",
    }
}

log_choice! {
    pub enum CallerTier {
        App = "app",
        Agent = "agent",
    }
}

log_choice! {
    pub enum DoneOrFailed {
        Done = "done",
        Failed = "failed",
    }
}

log_choice! {
    pub enum FailWhy {
        Pty = "pty",
        Timeout = "timeout",
        DialogOpen = "dialog_open",
        QuestionGone = "question_gone",
        QuestionMismatch = "question_mismatch",
        NotPermission = "not_permission",
        PermissionMismatch = "permission_mismatch",
        NoAlways = "no_always",
        PermissionStillOpen = "permission_still_open",
        NoInput = "no_input",
        NotCleared = "not_cleared",
        NotTaken = "not_taken",
        NotSent = "not_sent",
        ActingOff = "acting_off",
        NoTerminal = "no_terminal",
        NotIdle = "not_idle",
        AttachFailed = "attach_failed",
        IoNotFound = "io_not_found",
        IoPermissionDenied = "io_permission_denied",
        IoOther = "io_other",
        SqliteBusy = "sqlite_busy",
        SqliteCorrupt = "sqlite_corrupt",
        Keychain = "keychain",
        Network = "network",
    }
}

log_choice! {
    pub enum ItemKind {
        SecretApproval = "secret_approval",
        SecretRequest = "secret_request",
        LimitNotice = "limit_notice",
        ActionFailed = "action_failed",
        Notice = "notice",
        SettingChange = "setting_change",
        LogWarning = "log_warning",
    }
}

log_choice! {
    pub enum ItemAction {
        AllowOnce = "allow_once",
        AllowAlways = "allow_always",
        Deny = "deny",
        Cancel = "cancel",
        Dismiss = "dismiss",
        Open = "open",
    }
}

log_choice! {
    pub enum SwitchName {
        Acting = "acting",
        Bridge = "bridge",
        Accounts = "accounts",
        Vault = "vault",
        Install = "install",
        Cluster = "cluster",
        Machines = "machines",
    }
}

log_choice! {
    pub enum SinkName {
        Audit = "audit",
        Activity = "activity",
        Diagnostics = "diagnostics",
        Metrics = "metrics",
    }
}

log_choice! {
    pub enum PruneReason {
        Retention = "retention",
        SizeCap = "size_cap",
        User = "user",
    }
}

log_choice! {
    pub enum Provider {
        Claude = "claude",
        ClaudeDesktop = "claude_desktop",
        Codex = "codex",
        Agy = "agy",
    }
}

log_choice! {
    pub enum LimitWindow {
        FiveHour = "five_hour",
        Weekly = "weekly",
        WeeklyOpus = "weekly_opus",
        WeeklySonnet = "weekly_sonnet",
    }
}

log_choice! {
    pub enum ToolName {
        Codex = "codex",
        Agy = "agy",
    }
}

log_choice! {
    pub enum ProcName {
        Daemon = "daemon",
        App = "app",
        Hook = "hook",
        Cli = "cli",
        Ptyhost = "ptyhost",
    }
}

log_choice! {
    pub enum AskThen {
        Asked = "asked",
        Found = "found",
        Answered = "answered",
        Delivered = "delivered",
        Dismissed = "dismissed",
        Expired = "expired",
        Gone = "gone",
        Refused = "refused",
        Restored = "restored",
    }
}

log_choice! {
    pub enum ConsentThen {
        Asked = "asked",
        Refused = "refused",
        Opened = "opened",
        Allowed = "allowed",
        Denied = "denied",
        Expired = "expired",
        Cancelled = "cancelled",
        Withdrawn = "withdrawn",
        Applied = "applied",
        Failed = "failed",
        Restored = "restored",
        HandedOver = "handed_over",
    }
}

log_choice! {
    pub enum ConsentKind {
        Setting = "setting",
        SecretRequest = "secret_request",
        Switch = "switch",
        Session = "session",
        SessionStop = "session_stop",
        Dispatch = "dispatch",
        Job = "job",
        JobStop = "job_stop",
        Cluster = "cluster",
        Account = "account",
        Machine = "machine",
        Install = "install",
        CostProfile = "cost_profile",
        Update = "update",
        Model = "model",
    }
}

log_choice! {
    pub enum ConsentAction {
        ClusterCreate = "cluster_create",
        ClusterInvite = "cluster_invite",
        ClusterRemove = "cluster_remove",
        ClusterMain = "cluster_main",
        ClusterFailover = "cluster_failover",
        ClusterMoveHome = "cluster_move_home",
        ClusterAllow = "cluster_allow",
        AccountSwitch = "account_switch",
        AccountAdd = "account_add",
        AccountRemove = "account_remove",
        AccountMoveAll = "account_move_all",
        LaneCreate = "lane_create",
        LaneRemove = "lane_remove",
        LaneSwitch = "lane_switch",
        SessionMove = "session_move",
        MachineAdd = "machine_add",
        MachineFix = "machine_fix",
        MachineUpdate = "machine_update",
        MachineRemove = "machine_remove",
        TailscaleNetwork = "tailscale_network",
        InstallSetUp = "install_set_up",
        CostSet = "cost_set",
        CostApply = "cost_apply",
        CostUndo = "cost_undo",
        ToolUpdate = "tool_update",
        ModelDownload = "model_download",
    }
}

log_choice! {
    pub enum ConsentAnswerOn {
        Mac = "mac",
        MacOrPhone = "mac_or_phone",
    }
}

log_choice! {
    pub enum AskerKind {
        Main = "main",
        Subagent = "subagent",
        Codex = "codex",
        Antigravity = "antigravity",
        Unknown = "unknown",
    }
}

log_choice! {
    pub enum AskMode {
        Background = "background",
        Wait = "wait",
    }
}

log_choice! {
    pub enum AgentTool {
        Claude = "claude",
        Codex = "codex",
        Antigravity = "antigravity",
        Shell = "shell",
        Unknown = "unknown",
    }
}

log_choice! {
    pub enum AgentResult {
        Ok = "ok",
        Refused = "refused",
        Failed = "failed",
        Pending = "pending",
        DryRun = "dry_run",
    }
}
