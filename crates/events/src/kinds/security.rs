use super::choices::{CallerTier, SwitchName};
use crate::field::{Count, Exe, Millis, Op, Version};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum RefusedWhy {
        Forbidden = "forbidden",
        PeerChanged = "peer_changed",
        NeedsApproval = "needs_approval",
        SwitchOff = "switch_off",
        NotDescendant = "not_descendant",
        AuditUnwritable = "audit_unwritable",
    }
}

log_choice! {
    pub enum PeerWhy {
        PeerUnknown = "peer_unknown",
        OtherUser = "other_user",
    }
}

log_choice! {
    pub enum SwitchCause {
        User = "user",
        RestoredAtLaunch = "restored_at_launch",
        DaemonStart = "daemon_start",
        FileSaidOnIgnored = "file_said_on_ignored",
        RestoreFailed = "restore_failed",
    }
}

log_choice! {
    pub enum DataRoot {
        Real = "real",
        Fixture = "fixture",
    }
}

log_choice! {
    pub enum StartAfter {
        CleanStop = "clean_stop",
        Unknown = "unknown",
    }
}

log_choice! {
    pub enum Sealing {
        Sealed = "sealed",
        Unsealed = "unsealed",
        Waiting = "waiting",
    }
}

log_choice! {
    pub enum StopCause {
        Quit = "quit",
        Signal = "signal",
        Replaced = "replaced",
        Update = "update",
    }
}

log_choice! {
    pub enum DetachCause {
        Quit = "quit",
        Dropped = "dropped",
    }
}

kinds! {
    group: Group::Security,
    ApiRefused = "api.refused", kv 1 {
        title: "Refused Requests",
        records: "A program asked the daemon for something it may not do, and was refused.",
        why: "Shows which program tried what and why it was stopped. An agent that keeps trying shows up here.",
        default: On,
        lock: Lock::Always("Every refusal is part of the audit trail."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            op: Op,
            exe: Option<Exe>,
            tier: CallerTier,
            why: RefusedWhy,
            switch: Option<SwitchName>,
        }
    }
    ApiUnknownPeer = "api.unknown_peer", kv 1 {
        title: "Unknown Programs",
        records: "A connection the daemon could not identify, or one that came from another user.",
        why: "Shows a program that is not one of Notchling's own trying to reach the daemon.",
        default: On,
        lock: Lock::Always("An unidentified caller is part of the audit trail."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Warn,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            why: PeerWhy,
            exe: Option<Exe>,
        }
    }
    SwitchChanged = "switch.changed", kv 1 {
        title: "Switches On and Off",
        records: "One of the five switches went on or off, and what caused it.",
        why: "A switch decides what Notchling may do on your Mac. This shows who turned each one on or off, and when.",
        default: On,
        lock: Lock::Always("Switches decide what agents may do, so every change is recorded."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            switch: SwitchName,
            on: bool,
            cause: SwitchCause,
        }
    }
    DaemonStarted = "daemon.started", kv 1 {
        title: "Daemon Started",
        records: "The daemon started: its version, the macOS version, whether it runs over real folders, and whether the Audit was sealed.",
        why: "Shows each restart and whether the one before it was a clean stop.",
        default: On,
        lock: Lock::Always("A restart can hide a gap, so every start is recorded."),
        sinks: AUDIT | ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            daemon: Version,
            macos: Version,
            data_root: DataRoot,
            after: StartAfter,
            sealing: Sealing,
        }
    }
    DaemonStopped = "daemon.stopped", kv 1 {
        title: "Daemon Stopped",
        records: "The daemon stopped, why, and how long it had been running.",
        why: "A start with no stop before it means the daemon was killed or crashed.",
        default: On,
        lock: Lock::Always("A clean stop is what tells a gap from a crash."),
        sinks: AUDIT | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            cause: StopCause,
            uptime_ms: Millis,
        }
    }
    AppAttached = "app.attached", kv 1 {
        title: "App Connected",
        records: "The app connected to the daemon with the app tier: its version, program and process id.",
        why: "Shows when Notchling's own app was attached, which is when approvals can be answered.",
        default: On,
        lock: Lock::Always("Approvals depend on the app being attached."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            version: Version,
            exe: Exe,
            pid: Count,
        }
    }
    AppDetached = "app.detached", kv 1 {
        title: "App Disconnected",
        records: "The app's connection ended, either because it quit or because it dropped, and how long it was attached.",
        why: "A dropped connection stands in for an app crash or a kill.",
        default: On,
        lock: Lock::Always("A gap in the app's presence is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            cause: DetachCause,
            attached_ms: Millis,
        }
    }
}
