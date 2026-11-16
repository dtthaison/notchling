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
pub const DIALOG_RECHECK_MS: i64 = 30_000;
pub const REPLY_WAIT_MS: i64 = 50_000;
pub const POSTMEN_MAX: usize = 32;

pub use gate::{gate, Facts, Gate, Job};
pub use model::{
    new_id, Change, InsertError, Kind, New, Row, State, Tool, Via, Why, MAX_ROWS, MAX_TEXT,
};
pub use outcome::{after, backoff_ms, After, Tried, TryEnd};
pub use refusal::{is_settled, BEING_SENT, IN_QUEUE, NO_MESSAGE};
pub use store::OutboxStore;
pub use verdict::{line_within, queued_verdict, verdict, QueuedVerdict, Verdict};

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
#[path = "store_tests.rs"]
mod store_tests;
#[cfg(test)]
#[path = "verdict_tests.rs"]
mod verdict_tests;
