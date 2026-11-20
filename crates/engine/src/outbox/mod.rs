mod answer;
mod gate;
mod model;
mod outcome;
mod refusal;
mod store;
mod verdict;

pub const QUEUE_LINE_MS: i64 = 5_000;
pub const VERIFY_WATCH_MS: i64 = 10_000;
pub const VERIFY_LOOK_MS: i64 = 30_000;
pub const ATTACH_IDLE_MS: i64 = 10_000;
pub const TURN_SETTLE_MS: i64 = 3_000;
pub const QUEUE_STALE_MS: i64 = 10_000;
pub const GONE_AFTER_MS: i64 = 60_000;
pub const GONE_RECHECK_MS: i64 = 10_000;
pub const UNREADABLE_AFTER_MS: i64 = 30_000;
pub const SLOW_READ_MS: i64 = 2_000;
pub const DONE_STOPS_AFTER_TRIES: u32 = 3;
pub const DIALOG_RECHECK_MS: i64 = 30_000;
pub const ANSWER_GRACE_MS: i64 = 3_000;
pub const ANSWER_RECHECK_MS: i64 = 5_000;
pub const ANSWER_CLOSED_READS: u32 = 3;
pub const ANSWER_HOLD_MS: i64 = 600_000;
pub const REPLY_WAIT_MS: i64 = 50_000;
pub const POSTMEN_MAX: usize = 32;
pub const UNSURE_HOLD_MS: i64 = 120_000;
pub const HOLD_LEASE_MS: i64 = 600_000;

pub use answer::{AnswerWatch, Seen};
pub use gate::{gate, idle_at_prompt, recount, Facts, Gate, Job};
pub use model::{
    new_id, Anchor, Change, InsertError, Kind, New, Row, State, Tool, Via, Why, MAX_ROWS, MAX_TEXT,
};
pub use outcome::{after, backoff_ms, After, Tried, TryEnd};
pub use refusal::{is_settled, BEING_SENT, CHANGED, IN_QUEUE, NO_MESSAGE};
pub use store::OutboxStore;
pub use verdict::{line_within, queued_verdict, verdict, QueuedVerdict, Verdict};

#[cfg(test)]
#[path = "answer_tests.rs"]
mod answer_tests;
#[cfg(test)]
#[path = "gate_answer_tests.rs"]
mod gate_answer_tests;
#[cfg(test)]
#[path = "gate_idle_tests.rs"]
mod gate_idle_tests;
#[cfg(test)]
#[path = "gate_tests.rs"]
mod gate_tests;
#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
#[cfg(test)]
#[path = "outcome_tests.rs"]
mod outcome_tests;
#[cfg(test)]
#[path = "refusal_tests.rs"]
mod refusal_tests;
#[cfg(test)]
#[path = "store_anchor_tests.rs"]
mod store_anchor_tests;
#[cfg(test)]
#[path = "store_aside_tests.rs"]
mod store_aside_tests;
#[cfg(test)]
#[path = "store_blank_tests.rs"]
mod store_blank_tests;
#[cfg(test)]
#[path = "store_done_tests.rs"]
mod store_done_tests;
#[cfg(test)]
#[path = "store_join_tests.rs"]
mod store_join_tests;
#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
#[cfg(test)]
#[path = "store_unsure_queue_tests.rs"]
mod store_unsure_queue_tests;
#[cfg(test)]
#[path = "store_unsure_tests.rs"]
mod store_unsure_tests;
#[cfg(test)]
#[path = "verdict_tests.rs"]
mod verdict_tests;
