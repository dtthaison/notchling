use crate::field::{Bytes, Count, ErrorCode, Millis, Op};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum StallPart {
        ChatRoot = "chat_root",
        Conversation = "conversation",
        Sidebar = "sidebar",
        Details = "details",
        Notch = "notch",
    }
}

kinds! {
    group: Group::Performance,
    PerfOp = "perf.op", kv 1 {
        title: "Operation Times",
        records: "How long each request took, per request and result, kept as a histogram of milliseconds.",
        why: "Shows which requests are slow without recording each one.",
        default: On,
        lock: Lock::Toggle,
        sinks: METRICS,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 7,
        fields {
            op: Op,
            code: Option<ErrorCode>,
            ms: Millis,
        }
    }
    PerfPoll = "perf.poll", kv 1 {
        title: "Background Checks",
        records: "How many times each quiet background check succeeded.",
        why: "Shows how busy the app's polling is, which a single request log would drown.",
        default: On,
        lock: Lock::Toggle,
        sinks: METRICS,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 7,
        fields {
            op: Op,
        }
    }
    PerfStall = "perf.stall", kv 1 {
        title: "Slow Moments in the App",
        records: "The app's main thread was busy for over 50 milliseconds: in which part, kept as a histogram.",
        why: "Shows where the app stutters, so a slow part can be found and fixed.",
        default: On,
        lock: Lock::Toggle,
        sinks: METRICS | DIAG,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::App],
        agents_read: AgentsRead::No,
        keep_days: 7,
        fields {
            part: StallPart,
            ms: Millis,
        }
    }
    PerfReload = "perf.reload", kv 1 {
        title: "Chat Reloads",
        records: "A Chat reload: how many bytes it parsed, how many rows it built and how long it took.",
        why: "Shows how much work Chat does to follow a long session.",
        default: On,
        lock: Lock::Toggle,
        sinks: METRICS,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::App],
        agents_read: AgentsRead::No,
        keep_days: 7,
        fields {
            bytes_parsed: Bytes,
            rows_built: Count,
            ms: Millis,
        }
    }
}
