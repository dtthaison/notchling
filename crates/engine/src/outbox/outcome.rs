use super::Why;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    Verify,
    Box(Why),
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

pub fn after(end: &TryEnd, tried: Tried, tries: u32, now_ms: i64) -> After {
    let retry = After::Wait {
        why: Why::Retry,
        next_ms: Some(now_ms + backoff_ms(tries)),
    };
    match end {
        TryEnd::Ok => After::Verify,
        TryEnd::NotIdle => After::Wait {
            why: Why::Turn,
            next_ms: None,
        },
        TryEnd::AttachFailed => retry,
        TryEnd::Failed(_) if tried.entered => After::Verify,
        TryEnd::Failed(failure) => match failure {
            Failure::Dialog => After::Wait {
                why: Why::Dialog,
                next_ms: None,
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
            | Failure::PermissionStillOpen => retry,
        },
    }
}
