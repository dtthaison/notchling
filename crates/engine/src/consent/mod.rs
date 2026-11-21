mod inbox_item;
pub mod kinds;
mod model;
pub mod rules;
mod store;

pub use inbox_item::{consent_of, detail_of, item, item_id, title_of, DETAIL_CHARS, TITLE_CHARS};
pub use kinds::{spec, KindSpec, Outcome, KINDS, SECRET_REQUEST, SETTING};
pub use model::{
    bucket_of, clean_reason, new_id, AnswerOn, AppliedBy, Consent, New, Refusal, State,
    COOLDOWN_MS, DEFAULT_EXPIRY_MS, HOUR_MS, IDEM_MS, LONG_ALL, LONG_EXPIRY_MS, LONG_PER_SESSION,
    MAX_EXPIRY_MS, MIN_EXPIRY_MS, OPENED_MIN_MS, OPENED_PER_HOUR, REASON_CHARS, ROOT_MAX_EXPIRY_MS,
    ROOT_PER_HOUR, ROOT_WAITING, UNVERIFIED_PER_HOUR, UNVERIFIED_WAITING, WAITING_ALL,
    WAITING_PER_SESSION,
};
pub use store::{ConsentStore, Opened, Restart, Taken};

#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
