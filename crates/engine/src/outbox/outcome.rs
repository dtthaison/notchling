use super::{Why, LOGIN_RECHECK_MS};
use crate::reply::exec::Failure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tried {
    pub pasted: bool,
    pub entered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryEnd {
    Ok,
    Failed(Failure),
    NotIdle,
    AttachFailed,
    Cut,
    Asked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    Verify,
    Box(Why),
    Unsure(Why),
    Wait { why: Why, next_ms: Option<i64> },
}

pub fn backoff_ms(tries: u32) -> i64 {
    match tries {
        0 | 1 => 30_000,
        2 => 60_000,
        3 => 120_000,
        _ => 300_000,
    }
}

pub fn forgiven(end: &TryEnd, tried: Tried) -> bool {
    matches!(end, TryEnd::Failed(Failure::Timeout)) && !tried.pasted && !tried.entered
}

pub fn after(end: &TryEnd, tried: Tried, tries: u32, now_ms: i64) -> After {
    let retry = After::Wait {
        why: Why::Retry,
        next_ms: Some(now_ms + backoff_ms(tries)),
    };
    match end {
        TryEnd::Cut => After::Unsure(Why::Restart),
        TryEnd::Ok => After::Verify,
        TryEnd::Asked => After::Wait {
            why: Why::Answer,
            next_ms: None,
        },
        _ if tried.entered => After::Verify,
        TryEnd::NotIdle => After::Wait {
            why: Why::Turn,
            next_ms: None,
        },
        TryEnd::AttachFailed => retry,
        TryEnd::Failed(failure) => match failure {
            Failure::Dialog => After::Wait {
                why: Why::Dialog,
                next_ms: None,
            },
            Failure::LoginNeeded => After::Wait {
                why: Why::LoginNeeded,
                next_ms: Some(now_ms + LOGIN_RECHECK_MS),
            },
            Failure::NotCleared => After::Box(Why::NotCleared),
            Failure::Mixed => After::Box(Why::Mixed),
            Failure::Pty(_)
            | Failure::Timeout
            | Failure::NoInput
            | Failure::NotTaken
            | Failure::NotSent
            | Failure::QuestionGone
            | Failure::QuestionMismatch
            | Failure::NotPermission
            | Failure::PermissionMismatch
            | Failure::NoAlways
            | Failure::PermissionStillOpen
            | Failure::Working
            | Failure::Wake(_) => retry,
        },
    }
}
