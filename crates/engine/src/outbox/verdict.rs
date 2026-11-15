use super::VERIFY_LOOK_MS;
use crate::transcript::sent::Seen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Sent,
    InQueue,
    Pending,
    Unsure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedVerdict {
    Sent,
    Stay,
    Unsure,
}

pub fn verdict(seen: Option<Seen>, before: Seen, since_return_ms: i64) -> Verdict {
    match seen {
        Some(now) if now.taken > before.taken => Verdict::Sent,
        Some(now) if now.enqueued > before.enqueued => Verdict::InQueue,
        _ if since_return_ms < VERIFY_LOOK_MS => Verdict::Pending,
        _ => Verdict::Unsure,
    }
}

pub fn queued_verdict(
    seen: Option<Seen>,
    before: Seen,
    still_listed: bool,
    gone_since_ms: Option<i64>,
    now_ms: i64,
) -> QueuedVerdict {
    if seen.is_some_and(|now| now.taken > before.taken) {
        return QueuedVerdict::Sent;
    }
    if still_listed {
        return QueuedVerdict::Stay;
    }
    match gone_since_ms {
        Some(since) if now_ms - since >= VERIFY_LOOK_MS => QueuedVerdict::Unsure,
        _ => QueuedVerdict::Stay,
    }
}

pub fn line_within(seen_at: Option<Seen>, before: Seen) -> bool {
    seen_at.is_some_and(|now| now.enqueued > before.enqueued || now.taken > before.taken)
}
