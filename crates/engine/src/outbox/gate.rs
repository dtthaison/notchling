use super::{
    Kind, Row, State, Why, DIALOG_RECHECK_MS, GONE_AFTER_MS, QUEUE_STALE_MS, TURN_SETTLE_MS,
};

const RECHECK_MS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Missing { since_ms: i64 },
    Stopped,
    Live { status: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub now_ms: i64,
    pub job: Job,
    pub dialog_on_screen: bool,
    pub queue_waiting: usize,
    pub safe: bool,
    pub turn_over_since_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Go { guarded: bool },
    Wait { why: Why, until_ms: i64 },
    NeedsUser(State),
}

pub fn gate(row: &Row, facts: &Facts) -> Gate {
    let now = facts.now_ms;
    let status = match &facts.job {
        Job::Missing { since_ms } => {
            if now - since_ms >= GONE_AFTER_MS {
                return Gate::NeedsUser(State::Gone);
            }
            return Gate::Wait {
                why: Why::Retry,
                until_ms: now + RECHECK_MS,
            };
        }
        Job::Stopped => return Gate::NeedsUser(State::Stopped),
        Job::Live { status } => status.as_deref(),
    };
    if status == Some("waiting") || facts.dialog_on_screen {
        return Gate::Wait {
            why: Why::Dialog,
            until_ms: now + DIALOG_RECHECK_MS,
        };
    }
    if let Some(next) = row.next_ms.filter(|next| *next > now) {
        return Gate::Wait {
            why: Why::Retry,
            until_ms: next,
        };
    }
    if row.kind == Kind::Command || facts.safe {
        return safe_rule(facts);
    }
    if facts.queue_waiting > 0 {
        if queue_is_stale(status, facts) {
            return Gate::Go { guarded: true };
        }
        return Gate::Wait {
            why: Why::Queue,
            until_ms: now + RECHECK_MS,
        };
    }
    Gate::Go { guarded: false }
}

fn queue_is_stale(status: Option<&str>, facts: &Facts) -> bool {
    matches!(status, Some("idle" | "shell"))
        && facts
            .turn_over_since_ms
            .is_some_and(|since| facts.now_ms - since >= QUEUE_STALE_MS)
}

fn safe_rule(facts: &Facts) -> Gate {
    match facts.turn_over_since_ms {
        Some(since) if facts.now_ms - since >= TURN_SETTLE_MS => Gate::Go { guarded: true },
        _ => Gate::Wait {
            why: Why::Turn,
            until_ms: facts.now_ms + RECHECK_MS,
        },
    }
}
