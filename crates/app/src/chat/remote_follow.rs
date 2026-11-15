use super::data::Session;
use super::remote::{is_remote, FOLLOW_EVERY_MS};

pub const FOLLOW_RETRY_MS: i64 = 5_000;

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
            State::Refused => FOLLOW_RETRY_MS,
        };
        now_ms.saturating_sub(self.at_ms) >= wait
    }

    pub fn begin(&mut self, row: &str, now_ms: i64) {
        self.row = row.to_owned();
        self.at_ms = now_ms;
        self.state = State::Sent;
    }

    pub fn finish(&mut self, row: &str, ok: bool, now_ms: i64) {
        if self.row != row || self.state != State::Sent {
            return;
        }
        self.at_ms = now_ms;
        self.state = if ok { State::Done } else { State::Refused };
    }
}
