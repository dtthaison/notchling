use super::choices::Answerer;
use crate::field::{Exe, List, Millis};
use crate::field_ref::{Device, Folder, Ref, Secret, Session};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum Approval {
        AlwaysAllow = "always_allow",
        Asked = "asked",
    }
}

log_choice! {
    pub enum SecretResult {
        Given = "given",
        Denied = "denied",
        Expired = "expired",
        Cancelled = "cancelled",
        NoApp = "no_app",
        Failed = "failed",
    }
}

log_choice! {
    pub enum RequestResult {
        Saved = "saved",
        Cancelled = "cancelled",
        Expired = "expired",
        Withdrawn = "withdrawn",
    }
}

log_choice! {
    pub enum EntryAction {
        Saved = "saved",
        Removed = "removed",
        AskFirstOn = "ask_first_on",
        AskFirstOff = "ask_first_off",
    }
}

kinds! {
    group: Group::Secrets,
    VaultSecretUsed = "vault.secret_used", kv 1 {
        title: "Secrets Given to Commands",
        records: "Each time a command an agent ran got a secret from the vault: which secrets, which session and folder, which program, and who allowed it.",
        why: "Shows which agent used which secret, and whether you, your phone or an Always Allow rule let it.",
        default: On,
        lock: Lock::Always("Every use of a secret is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 0,
        fields {
            secrets: List<Ref<Secret>>,
            session: Option<Ref<Session>>,
            folder: Option<Ref<Folder>>,
            command_exe: Option<Exe>,
            approval: Approval,
            answered_by: Option<Answerer>,
            device: Option<Ref<Device>>,
            result: SecretResult,
            waited_ms: Option<Millis>,
        }
    }
    VaultSecretRequested = "vault.secret_requested", kv 1 {
        title: "Secrets Asked For",
        records: "An agent asked you to type a new secret: its name, the session and folder, and how the request ended.",
        why: "Shows which agent asked for which credential, and whether you gave it, refused or let it expire.",
        default: On,
        lock: Lock::Always("Every request for a secret is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 0,
        fields {
            name: Ref<Secret>,
            session: Option<Ref<Session>>,
            folder: Option<Ref<Folder>>,
            result: RequestResult,
            answered_by: Option<Answerer>,
        }
    }
    VaultEntryChanged = "vault.entry_changed", kv 1 {
        title: "Vault Entries Saved or Removed",
        records: "A vault entry was saved or removed, or its ask-first rule changed. The value is never recorded.",
        why: "Shows how the vault's contents changed over time, without ever showing a secret.",
        default: On,
        lock: Lock::Always("Changes to the vault are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            name: Ref<Secret>,
            action: EntryAction,
        }
    }
}
