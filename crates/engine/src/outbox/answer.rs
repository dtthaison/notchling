use super::{ANSWER_CLOSED_READS, ANSWER_RECHECK_MS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Open,
    Closed,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerWatch {
    row: String,
    closed: u32,
    look_after: i64,
    was_waiting: bool,
}

impl AnswerWatch {
    pub fn open(
        &mut self,
        row: &str,
        status: Option<&str>,
        now_ms: i64,
        look: impl FnOnce() -> Seen,
    ) -> bool {
        if self.row != row {
            *self = AnswerWatch {
                row: row.to_owned(),
                ..AnswerWatch::default()
            };
        }
        if status == Some("waiting") {
            self.was_waiting = true;
            self.closed = 0;
            return true;
        }
        if now_ms < self.look_after {
            return true;
        }
        match look() {
            Seen::Open => {
                self.closed = 0;
                self.look_after = now_ms + ANSWER_RECHECK_MS;
                true
            }
            Seen::Unknown => {
                self.look_after = now_ms + ANSWER_RECHECK_MS;
                true
            }
            Seen::Closed => {
                self.closed += 1;
                let answered = self.was_waiting && status == Some("idle");
                !answered && self.closed < ANSWER_CLOSED_READS
            }
        }
    }
}
