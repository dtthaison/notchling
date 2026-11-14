use super::choices::{ItemAction, ItemKind};
use crate::field::Millis;
use crate::field_ref::{Device, Item, Ref, Session};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum ItemState {
        Answered = "answered",
        Expired = "expired",
        Withdrawn = "withdrawn",
    }
}

log_choice! {
    pub enum InboxBy {
        Mac = "mac",
        Phone = "phone",
    }
}

kinds! {
    group: Group::Inbox,
    InboxRaised = "inbox.raised", kv 1 {
        title: "Items That Need You",
        records: "Something needed your answer and was put in the Inbox: what kind of item it was and which session it came from.",
        why: "Shows what asked for your attention and when.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            item_kind: ItemKind,
            item: Ref<Item>,
            session: Option<Ref<Session>>,
        }
    }
    InboxClosed = "inbox.closed", kv 1 {
        title: "Items Answered or Closed",
        records: "An Inbox item was answered, expired or withdrawn, what the answer was, and whether the Mac or a phone gave it.",
        why: "Shows how each approval ended and who allowed it.",
        default: On,
        lock: Lock::Toggle,
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            item_kind: ItemKind,
            item: Ref<Item>,
            state: ItemState,
            action: Option<ItemAction>,
            by: Option<InboxBy>,
            device: Option<Ref<Device>>,
            waited_ms: Option<Millis>,
        }
    }
}
