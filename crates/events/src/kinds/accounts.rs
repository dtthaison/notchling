use super::choices::{DoneOrFailed, LimitWindow, Provider};
use crate::field::{Count, Money, Percent, Time, Tokens};
use crate::field_ref::{Account, Lane, Model, Ref};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum LoginAction {
        Added = "added",
        Removed = "removed",
        RenewedSaved = "renewed_saved",
    }
}

log_choice! {
    pub enum AccountCause {
        User = "user",
        LimitAuto = "limit_auto",
        MoveAll = "move_all",
        Lane = "lane",
    }
}

log_choice! {
    pub enum LaneAction {
        Created = "created",
        Removed = "removed",
        Switched = "switched",
    }
}

kinds! {
    group: Group::Accounts,
    AccountLogin = "account.login", kv 1 {
        title: "Logins Added or Removed",
        records: "A login was added, removed or renewed, for which tool, and whether it worked.",
        why: "Shows every change to the logins Notchling keeps, since they decide which account an agent uses.",
        default: On,
        lock: Lock::Always("Changes to logins are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            provider: Provider,
            account: Ref<Account>,
            action: LoginAction,
            result: DoneOrFailed,
        }
    }
    AccountSwitched = "account.switched", kv 1 {
        title: "Account Switches",
        records: "The login in use changed from one account to another, and what caused it.",
        why: "Shows when and why your sessions moved to another account, such as a limit reached.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 90,
        fields {
            provider: Provider,
            from: Option<Ref<Account>>,
            to: Ref<Account>,
            cause: AccountCause,
            window: Option<LimitWindow>,
            used_pct: Option<Percent>,
            result: DoneOrFailed,
        }
    }
    LimitsCrossed = "limits.crossed", kv 1 {
        title: "Limit Warnings",
        records: "An account passed 50, 75, 90 or 100 percent of a usage limit, and when the limit resets.",
        why: "Shows how close each account came to its limit, and when.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            provider: Provider,
            account: Ref<Account>,
            window: LimitWindow,
            used_pct: Percent,
            resets_at: Option<Time>,
        }
    }
    UsageDaily = "usage.daily", kv 1 {
        title: "Daily Use and Cost",
        records: "Once a day for each account and model: tokens used, and the cost when the model has a price.",
        why: "Keeps a long history of what each account and model cost, for the cost view.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 365,
        fields {
            day: Count,
            provider: Provider,
            account: Ref<Account>,
            model: Ref<Model>,
            tokens_in: Tokens,
            tokens_out: Tokens,
            cache_read: Tokens,
            cache_write: Tokens,
            cost: Money,
            priced: bool,
        }
    }
    LaneChanged = "lane.changed", kv 1 {
        title: "Lanes",
        records: "A lane was created, removed or switched to another account.",
        why: "Shows how your lanes, the separate account setups, changed over time.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            lane: Ref<Lane>,
            action: LaneAction,
            account: Option<Ref<Account>>,
        }
    }
}
