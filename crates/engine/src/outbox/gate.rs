use super::{
    Kind, Row, State, Why, ANSWER_GRACE_MS, ANSWER_HOLD_MS, DIALOG_RECHECK_MS,
    DONE_STOPS_AFTER_TRIES, GONE_AFTER_MS, QUEUE_STALE_MS, TURN_SETTLE_MS, UNREADABLE_AFTER_MS,
};

const RECHECK_MS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Missing { since_ms: i64 },
    Stopped,
    Unknown { since_ms: i64 },
    Done { failed: u32 },
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
    Unsure(Why),
    Delivered,
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
        Job::Unknown { since_ms } => {
            let age = now - since_ms;
            let why = if age >= UNREADABLE_AFTER_MS {
                Why::Unreadable
            } else {
                Why::Checking
            };
            let mut until_ms = now + unknown_recheck_ms(age);
            if why == Why::Checking {
                until_ms = until_ms.min(since_ms + UNREADABLE_AFTER_MS);
            }
            return Gate::Wait { why, until_ms };
        }
        Job::Done { failed } if *failed >= DONE_STOPS_AFTER_TRIES => {
            return Gate::NeedsUser(State::Stopped);
        }
        Job::Done { .. } => None,
        Job::Live { status } => status.as_deref(),
    };
    if row.why == Some(Why::Answer) {
        return answer_gate(row, status, facts);
    }
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

fn unknown_recheck_ms(age_ms: i64) -> i64 {
    match age_ms {
        ..5_000 => RECHECK_MS,
        5_000..15_000 => 2 * RECHECK_MS,
        15_000..30_000 => 5 * RECHECK_MS,
        30_000..60_000 => 15 * RECHECK_MS,
        _ => 30 * RECHECK_MS,
    }
}

pub fn idle_at_prompt(facts: &Facts) -> bool {
    matches!(&facts.job, Job::Live { status: Some(status) } if status == "idle")
        && !facts.dialog_on_screen
        && !facts.safe
        && facts.queue_waiting == 0
}

fn answer_gate(row: &Row, status: Option<&str>, facts: &Facts) -> Gate {
    let held = facts.now_ms - row.updated_ms;
    let asked = status == Some("waiting") || facts.dialog_on_screen || held < ANSWER_GRACE_MS;
    if !asked {
        return Gate::Delivered;
    }
    if held >= ANSWER_HOLD_MS {
        return Gate::Unsure(Why::Answer);
    }
    Gate::Wait {
        why: Why::Answer,
        until_ms: facts.now_ms + RECHECK_MS,
    }
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
