use super::choices::{DoneOrFailed, ItemAction, ItemKind};
use crate::field::{Count, Millis};
use crate::field_ref::{Device, Ref, Session};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

log_choice! {
    pub enum PairAction {
        Paired = "paired",
        Revoked = "revoked",
        Refused = "refused",
    }
}

log_choice! {
    pub enum PairVia {
        Qr = "qr",
        Code = "code",
    }
}

log_choice! {
    pub enum LinkVia {
        Lan = "lan",
        Tunnel = "tunnel",
    }
}

log_choice! {
    pub enum ConnectionAction {
        Connected = "connected",
        Disconnected = "disconnected",
    }
}

log_choice! {
    pub enum PhoneOp {
        Message = "message",
        SendNow = "send_now",
        Answer = "answer",
        NewSession = "new_session",
        Stop = "stop",
        Image = "image",
    }
}

log_choice! {
    pub enum PhoneAnswerResult {
        Taken = "taken",
        ActingOff = "acting_off",
        Invalid = "invalid",
        Open = "open",
        NotFound = "not_found",
        NotTaken = "not_taken",
    }
}

log_choice! {
    pub enum PhoneView {
        Sessions = "sessions",
        Conversation = "conversation",
        Terminal = "terminal",
        PageImage = "page_image",
    }
}

log_choice! {
    pub enum TunnelAction {
        KeysFetched = "keys_fetched",
        TokenRefused = "token_refused",
        Connected = "connected",
    }
}

kinds! {
    group: Group::Phone,
    PhonePaired = "phone.paired", kv 1 {
        title: "Phones Paired or Removed",
        records: "A phone was paired, revoked, or refused when it tried to pair.",
        why: "Shows which phones can reach your Mac, and when each one was added or removed.",
        default: On,
        lock: Lock::Always("Which phones may reach the Mac is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            device: Ref<Device>,
            action: PairAction,
            via: Option<PairVia>,
        }
    }
    PhoneConnected = "phone.connected", kv 1 {
        title: "Phone Connections",
        records: "A phone connected or disconnected, over the local network or the tunnel, how long it stayed and how many requests it made.",
        why: "Shows when a phone was talking to your Mac. Reconnects within five minutes fold into one row.",
        default: On,
        lock: Lock::Always("Phone access to the Mac is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            device: Ref<Device>,
            via: LinkVia,
            action: ConnectionAction,
            duration_ms: Option<Millis>,
            ops: Option<Count>,
        }
    }
    PhoneActed = "phone.acted", kv 1 {
        title: "What Phones Did",
        records: "A phone sent a message, answered a prompt, started or stopped a session, or sent an image.",
        why: "Shows what each paired phone did on your Mac. Message text is never recorded.",
        default: On,
        lock: Lock::Always("Actions from a phone are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            device: Ref<Device>,
            op: PhoneOp,
            session: Option<Ref<Session>>,
            chars: Option<Count>,
        }
    }
    PhoneAnswered = "phone.answered", kv 1 {
        title: "Phone Answers",
        records: "A phone answered an Inbox item, and whether the answer was taken.",
        why: "Shows approvals given from a phone, including answers that were too late or refused.",
        default: On,
        lock: Lock::Always("Approvals from a phone are part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 0,
        fields {
            device: Ref<Device>,
            item_kind: Option<ItemKind>,
            action: Option<ItemAction>,
            result: PhoneAnswerResult,
        }
    }
    PhoneViewed = "phone.viewed", kv 1 {
        title: "What Phones Read",
        records: "A phone opened the session list, a conversation, a terminal or a page image.",
        why: "Shows what a phone looked at. It is off by default because it is frequent.",
        default: Off,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Debug,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            device: Ref<Device>,
            view: PhoneView,
            session: Option<Ref<Session>>,
        }
    }
    BridgeTunnel = "bridge.tunnel", kv 1 {
        title: "Tunnel",
        records: "The tunnel to your phone fetched its keys, refused a token or connected, and whether that worked.",
        why: "Helps explain why a phone could not reach your Mac from outside your network.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            action: TunnelAction,
            result: DoneOrFailed,
        }
    }
}
