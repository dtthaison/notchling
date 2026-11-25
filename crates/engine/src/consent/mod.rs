mod inbox_item;
pub mod kinds;
mod model;
pub mod rules;
mod store;
mod worker_switch;

pub use inbox_item::{consent_of, detail_of, item, item_id, title_of, DETAIL_CHARS, TITLE_CHARS};
pub use kinds::{
    spec, KindSpec, Outcome, ACCOUNT, CLUSTER, COST_PROFILE, DISPATCH, INSTALL, JOB, JOB_STOP,
    KINDS, MACHINE, MODEL, SECRET_REQUEST, SESSION, SESSION_STOP, SETTING, SWITCH, UPDATE,
};
pub use model::{
    bucket_of, clean_reason, new_id, AnswerOn, AppliedBy, Consent, New, Refusal, State,
    COOLDOWN_MS, DEFAULT_EXPIRY_MS, HOUR_MS, IDEM_MS, LONG_ALL, LONG_EXPIRY_MS, LONG_PER_SESSION,
    MAX_EXPIRY_MS, MIN_EXPIRY_MS, OPENED_MIN_MS, OPENED_PER_HOUR, REASON_CHARS, ROOT_MAX_EXPIRY_MS,
    ROOT_PER_HOUR, ROOT_WAITING, UNVERIFIED_BUCKET, WAITING_ALL, WAITING_PER_SESSION,
};
pub use store::{ConsentStore, Opened, Restart, Taken};
pub use worker_switch::{member_set_args, worker_switch, WorkerSwitch};

#[cfg(test)]
#[path = "job_kinds_tests.rs"]
mod job_kinds_tests;
#[cfg(test)]
#[path = "p8_kinds_tests.rs"]
mod p8_kinds_tests;
#[cfg(test)]
#[path = "rules_tests.rs"]
mod rules_tests;
#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
