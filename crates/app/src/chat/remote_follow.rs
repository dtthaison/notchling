use super::data::Session;
use super::remote::{is_remote, FOLLOW_EVERY_MS};

pub const FOLLOW_RETRY_MS: i64 = 5_000;
pub const FOLLOW_RETRY_SLOW_MS: i64 = 15_000;
pub const QUICK_REFUSALS: u32 = 3;
pub const SLOW_REFUSALS: u32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Sent,
    Done,
    Refused,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Follow {
    row: String,
    at_ms: i64,
    state: State,
    request: u64,
    refusals: u32,
}

pub fn retry_wait(refusals: u32) -> i64 {
    if refusals <= QUICK_REFUSALS {
        FOLLOW_RETRY_MS
    } else if refusals <= SLOW_REFUSALS {
        FOLLOW_RETRY_SLOW_MS
    } else {
        FOLLOW_EVERY_MS
    }
}

impl Follow {
    pub fn due(&self, selected: Option<&Session>, now_ms: i64) -> bool {
        let Some(session) = selected.filter(|s| is_remote(&s.agent)) else {
            return false;
        };
        if self.row != session.agent.id {
            return true;
        }
        let wait = match self.state {
            State::Idle => return true,
            State::Sent | State::Done => FOLLOW_EVERY_MS,
            State::Refused => retry_wait(self.refusals),
        };
        now_ms.saturating_sub(self.at_ms) >= wait
    }

    pub fn begin(&mut self, row: &str, now_ms: i64) -> u64 {
        if self.row != row {
            self.refusals = 0;
        }
        self.row = row.to_owned();
        self.at_ms = now_ms;
        self.state = State::Sent;
        self.request = self.request.wrapping_add(1);
        self.request
    }

    pub fn finish(&mut self, request: u64, ok: bool, now_ms: i64) {
        if self.request != request || self.state != State::Sent {
            return;
        }
        self.at_ms = now_ms;
        if ok {
            self.refusals = 0;
            self.state = State::Done;
        } else {
            self.refusals = self.refusals.saturating_add(1);
            self.state = State::Refused;
        }
    }
}
