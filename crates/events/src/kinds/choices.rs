log_choice! {
    pub enum Answerer {
        Mac = "mac",
        Phone = "phone",
        NoOne = "none",
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
