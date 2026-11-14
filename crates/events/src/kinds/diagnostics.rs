use super::choices::{CallerTier, FailWhy, ProcName};
use crate::field::{CodeLoc, Count, ErrorCode, Hex, Millis, Op, Version};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum ProcStopCause {
        Quit = "quit",
        Signal = "signal",
    }
}

log_choice! {
    pub enum CallResult {
        Ok = "ok",
        Error = "error",
    }
}

log_choice! {
    pub enum UnreachableWhy {
        NoSocket = "no_socket",
        Refused = "refused",
        Timeout = "timeout",
        UntrustedServer = "untrusted_server",
        Version = "version",
    }
}

kinds! {
    group: Group::Diagnostics,
    ProcessStarted = "process.started", kv 1 {
        title: "App and Terminal Host Started",
        records: "The app or the terminal host started, with its version. Never sealed.",
        why: "Shows each start, which helps match a problem to the build that had it.",
        default: On,
        lock: Lock::Always("Starts and stops are how a crash or a gap is told apart."),
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::App, Proc::Ptyhost, Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            proc: ProcName,
            version: Version,
            macos: Option<Version>,
        }
    }
    ProcessStopped = "process.stopped", kv 1 {
        title: "App and Terminal Host Stopped",
        records: "The app or the terminal host stopped, why, and how long it had been running. Never sealed.",
        why: "A start with no stop before it means the process was killed or crashed.",
        default: On,
        lock: Lock::Always("Starts and stops are how a crash or a gap is told apart."),
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::App, Proc::Ptyhost, Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            proc: ProcName,
            cause: ProcStopCause,
            uptime_ms: Millis,
        }
    }
    ProcessCrashed = "process.crashed", kv 1 {
        title: "Crashes",
        records: "A process crashed, read from its crash file at the next start: where, and a hash of the message. The stack stays in the crash file.",
        why: "Shows that a crash happened and where, without copying the message, which can hold paths.",
        default: On,
        lock: Lock::Always("A crash must never go unnoticed."),
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Error,
        emitters: &[Proc::App, Proc::Ptyhost, Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            proc: ProcName,
            at: CodeLoc,
            message_hash: Hex,
            main_thread: bool,
        }
    }
    OpCalled = "op.called", kv 1 {
        title: "Other Requests From the App",
        records: "A request from the app that has no kind of its own: which one, whether it worked and how long it took.",
        why: "Covers every other request the app makes, so none goes unrecorded. Quiet polls are only counted.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            op: Op,
            tier: CallerTier,
            result: CallResult,
            code: Option<ErrorCode>,
            ms: Millis,
        }
    }
    OpFailed = "op.failed", kv 1 {
        title: "Failed Operations",
        records: "A request failed: which one, the error code, the area's own reason and how long it took.",
        why: "Helps find what is going wrong when something does not work.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon, Proc::App],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            op: Op,
            code: ErrorCode,
            why: FailWhy,
            duration_ms: Millis,
        }
    }
    DaemonUnreachable = "daemon.unreachable", kv 1 {
        title: "Daemon Not Reachable",
        records: "The app could not reach the daemon, why, and how many times in a row.",
        why: "Explains why the app showed nothing or acted slowly.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::App],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            why: UnreachableWhy,
            count: Count,
        }
    }
    HostIdentity = "host.identity", kv 1 {
        title: "Machine Id",
        records: "This Mac's id changed, which happens when the bridge certificate is first made.",
        why: "Keeps logs from before and after the change tied to the same Mac.",
        default: On,
        lock: Lock::Always("Which machine wrote an entry is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            old: Hex,
            new: Hex,
        }
    }
}
