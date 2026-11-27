use crate::field::FieldDesc;
use std::ops::BitOr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Security,
    Secrets,
    Agents,
    Inbox,
    Accounts,
    Phone,
    Settings,
    Diagnostics,
    Performance,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::Security,
        Group::Secrets,
        Group::Agents,
        Group::Inbox,
        Group::Accounts,
        Group::Phone,
        Group::Settings,
        Group::Diagnostics,
        Group::Performance,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Security => "security",
            Group::Secrets => "secrets",
            Group::Agents => "agents",
            Group::Inbox => "inbox",
            Group::Accounts => "accounts",
            Group::Phone => "phone",
            Group::Settings => "settings",
            Group::Diagnostics => "diagnostics",
            Group::Performance => "performance",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Group::Security => "Security",
            Group::Secrets => "Secrets",
            Group::Agents => "Agents",
            Group::Inbox => "Inbox and Approvals",
            Group::Accounts => "Accounts and Limits",
            Group::Phone => "Phone",
            Group::Settings => "Settings",
            Group::Diagnostics => "Diagnostics",
            Group::Performance => "Performance",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultState {
    On,
    Off,
}

impl DefaultState {
    pub fn name(self) -> &'static str {
        match self {
            DefaultState::On => "on",
            DefaultState::Off => "off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    Always(&'static str),
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sinks(u8);

impl Sinks {
    pub const NONE: Sinks = Sinks(0);
    pub const AUDIT: Sinks = Sinks(1);
    pub const ACTIVITY: Sinks = Sinks(2);
    pub const DIAG: Sinks = Sinks(4);
    pub const METRICS: Sinks = Sinks(8);

    pub const fn with(self, other: Sinks) -> Sinks {
        Sinks(self.0 | other.0)
    }

    pub const fn has(self, other: Sinks) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn names(self) -> Vec<&'static str> {
        [
            (Sinks::AUDIT, "audit"),
            (Sinks::ACTIVITY, "activity"),
            (Sinks::DIAG, "diagnostics"),
            (Sinks::METRICS, "metrics"),
        ]
        .into_iter()
        .filter(|(sink, _)| self.has(*sink))
        .map(|(_, name)| name)
        .collect()
    }
}

impl BitOr for Sinks {
    type Output = Sinks;

    fn bitor(self, other: Sinks) -> Sinks {
        self.with(other)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    Push,
    CommitFromAgent,
}

impl Durability {
    pub fn name(self) -> &'static str {
        match self {
            Durability::Push => "push",
            Durability::CommitFromAgent => "commit_from_agent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proc {
    Daemon,
    App,
    Hook,
    Cli,
    Ptyhost,
}

impl Proc {
    pub fn name(self) -> &'static str {
        match self {
            Proc::Daemon => "daemon",
            Proc::App => "app",
            Proc::Hook => "hook",
            Proc::Cli => "cli",
            Proc::Ptyhost => "ptyhost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentsRead {
    No,
    Own,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Refused,
    Failed,
    Waiting,
    Info,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
            Outcome::Failed => "failed",
            Outcome::Waiting => "waiting",
            Outcome::Info => "info",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: &'static str,
    pub desc: FieldDesc,
}

#[derive(Debug)]
pub struct KindSpec {
    pub name: &'static str,
    pub kv: u32,
    pub group: Group,
    pub title: &'static str,
    pub records: &'static str,
    pub why: &'static str,
    pub default: DefaultState,
    pub lock: Lock,
    pub sinks: Sinks,
    pub durability: Durability,
    pub level: Level,
    pub emitters: &'static [Proc],
    pub agents_read: AgentsRead,
    pub keep_days: u16,
    pub retired: bool,
    pub fields: &'static [FieldSpec],
}

impl PartialEq for KindSpec {
    fn eq(&self, other: &KindSpec) -> bool {
        self.name == other.name
    }
}
