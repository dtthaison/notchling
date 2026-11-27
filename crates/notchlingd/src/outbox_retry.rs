use super::{baseline, Postman, TICK_MS};
use notchling_engine::outbox::{
    backoff_ms, typed_command, Row, State, TryEnd, Verdict, Why, NOT_SENT_RETRIES,
    UNREADABLE_AFTER_MS,
};
use notchling_engine::reply::exec::Failure;
use notchling_events::kinds::agents::OutboxThen;

pub(super) fn ignored_command(taken: &Row, end: &TryEnd, note: &Option<String>) -> bool {
    *end == TryEnd::Failed(Failure::NotSent)
        && typed_command(taken.tool, &taken.text)
        && note.is_some()
}

impl Postman {
    pub(super) fn landed_before_retry(&mut self, row: &Row, now: i64) -> Option<Option<i64>> {
        let courier = self.courier();
        let tried = row.tries > 0 && row.seen_before.is_some() && row.queued_before.is_some();
        if !tried || !courier.verifies() {
            return None;
        }
        let (before, verifier) = (baseline(row), self.inner.verifier.clone());
        let landed = verifier.found_late(row, before);
        if !landed && !row.unsent {
            return None;
        }
        if !landed {
            let since = row.next_ms.unwrap_or(row.updated_ms).max(row.updated_ms);
            match courier.text_left(row) {
                Some(true) => return None,
                None if now < since + UNREADABLE_AFTER_MS => return Some(Some(now + TICK_MS)),
                _ => {}
            }
        }
        let settled = match landed {
            true => Verdict::Sent,
            false => verifier.wait(row, before, (self.inner.clock)()),
        };
        let at = |moved: bool| Some((!moved).then_some(now + TICK_MS));
        match settled {
            Verdict::Sent => at(self.sent(row)),
            Verdict::InQueue => {
                let then = Some(OutboxThen::InQueue);
                at(self.moved(row, row.state, State::InQueue, None, None, then))
            }
            Verdict::Unsure => {
                let then = Some(OutboxThen::Unsure);
                let why = Some(Why::NotFound);
                at(self.moved(row, row.state, State::Unsure, why, None, then))
            }
            Verdict::Pending => Some(Some(now + TICK_MS)),
        }
    }

    pub(super) fn left_in_box(&mut self, taken: &Row, note: Option<String>) -> bool {
        self.inner.ops.set_note(&taken.id, note);
        let then = Some(OutboxThen::Box);
        let why = Some(Why::NotCleared);
        self.moved(taken, State::Sending, State::Box, why, None, then)
    }

    pub(super) fn left_unsent(
        &mut self,
        taken: &Row,
        end: &TryEnd,
        note: &Option<String>,
        ended: i64,
    ) -> bool {
        if *end != TryEnd::Failed(Failure::NotSent) || taken.tries > NOT_SENT_RETRIES {
            return false;
        }
        self.inner.ops.set_note(&taken.id, note.clone());
        let _ = self.inner.ops.store().set_unsent(&taken.id, true);
        let next = Some(ended + backoff_ms(taken.tries));
        let (from, to) = (State::Sending, State::Waiting);
        self.moved(taken, from, to, Some(Why::Retry), next, None);
        true
    }
}

#[cfg(test)]
#[path = "outbox_retry_tests.rs"]
mod tests;
