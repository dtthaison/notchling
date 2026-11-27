use super::choices::{PruneReason, SinkName};
use crate::field::{Count, Hex, KindName, List, Seal, Time};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum CheckResult {
        Intact = "intact",
        Broken = "broken",
        Gap = "gap",
        TailMissing = "tail_missing",
        FileMissing = "file_missing",
        Unanchored = "unanchored",
        Unsealed = "unsealed",
        Resumed = "resumed",
    }
}

log_choice! {
    pub enum ChainReason {
        FirstRun = "first_run",
        KeyMissing = "key_missing",
        KeyDenied = "key_denied",
        User = "user",
    }
}

log_choice! {
    pub enum DetailLevel {
        Normal = "normal",
        Detailed = "detailed",
    }
}

log_choice! {
    pub enum LogSink {
        Activity = "activity",
        Diagnostics = "diagnostics",
    }
}

log_choice! {
    pub enum DropReason {
        QueueFull = "queue_full",
        RateLimited = "rate_limited",
        WriteFailed = "write_failed",
        SpoolFull = "spool_full",
        Invalid = "invalid",
    }
}

kinds! {
    group: Group::Security,
    AuditChecked = "audit.checked", kv 1 {
        title: "Audit Log Checks",
        records: "The Audit's seals were checked, and whether the chain is intact, broken or has a gap.",
        why: "Shows that the record of what agents did has not been edited, cut or reordered.",
        default: On,
        lock: Lock::Always("The check is what makes the Audit tamper-evident."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            result: CheckResult,
            from_n: Option<Count>,
            to_n: Option<Count>,
            first_bad_n: Option<Count>,
        }
    }
    AuditChainStarted = "audit.chain_started", kv 1 {
        title: "Audit Log Started",
        records: "A new key and chain began, why, and the last sealed line of the chain before it, if any.",
        why: "Shows where one sealed chain ends and the next begins, so a break is never silent.",
        default: On,
        lock: Lock::Always("A new chain must be visible in the old one's record."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            reason: ChainReason,
            previous_head_n: Option<Count>,
            previous_head_seal: Option<Seal>,
            legacy_files: Option<Count>,
            legacy_lines: Option<Count>,
            legacy_sha: List<Hex>,
        }
    }
    AuditPruned = "audit.pruned", kv 1 {
        title: "Old Audit Months Removed",
        records: "Old Audit months were deleted: through which month, the last seal, how many lines, and every deletion you ever made.",
        why: "Deleting history leaves a sealed note, so a removed month is explained and never looks like tampering.",
        default: On,
        lock: Lock::Always("Removed history must leave a sealed trace."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            through_month: Count,
            last_seal: Seal,
            lines: Count,
            reason: PruneReason,
            user_deleted_months: List<Count>,
            user_deleted_at: List<Time>,
            user_deleted_count: Count,
        }
    }
    LogSettingChanged = "log.setting_changed", kv 1 {
        title: "Logging Settings Changed",
        records: "A logging setting changed: which kind or sink, on or off, how long to keep it, or how much detail.",
        why: "Turning logging off is itself recorded, so nobody can quiet the log without leaving a trace.",
        default: On,
        lock: Lock::Always("Changes to logging must be logged."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            target: Option<KindName>,
            sink: Option<SinkName>,
            on: Option<bool>,
            keep_days: Option<Count>,
            detail: Option<DetailLevel>,
            settings_hash: Hex,
        }
    }
    LogSettingsRestored = "log.settings_restored", kv 1 {
        title: "Logging Settings Reset",
        records: "The settings on disk did not match the Audit's record, so the defaults came back.",
        why: "Shows that someone edited the logging settings file outside the app.",
        default: On,
        lock: Lock::Always("An edit outside the app must be visible."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            kinds_changed: Count,
        }
    }
    LogPruned = "log.pruned", kv 1 {
        title: "Old Activity and Diagnostics Removed",
        records: "Old Activity rows or Diagnostics files were removed, the span they covered, and how many entries.",
        why: "Explains why older entries are gone: the retention period, the size cap or your own deletion.",
        default: On,
        lock: Lock::Always("Removed history must leave a trace."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            sink: LogSink,
            from: Time,
            to: Time,
            entries: Count,
            reason: PruneReason,
        }
    }
    LogDropped = "log.dropped", kv 1 {
        title: "Entries Not Recorded",
        records: "Entries could not be recorded, which sink missed them, how many, and why.",
        why: "A busy or failing log says so here instead of losing entries without a word.",
        default: On,
        lock: Lock::Always("A gap in the log must be visible."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            sink: SinkName,
            count: Count,
            reason: DropReason,
        }
    }
    LogSaltRotated = "log.salt_rotated", kv 1 {
        title: "Diagnostics Hashes Changed",
        records: "The salt file was missing or unreadable and a new one was made.",
        why: "Hashed names in Diagnostics from before and after this point no longer match each other.",
        default: On,
        lock: Lock::Always("A new salt changes what the hashes mean."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            old_id: Option<Hex>,
            new_id: Hex,
        }
    }
}
