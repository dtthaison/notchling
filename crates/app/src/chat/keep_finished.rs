use notchling_engine::settings::{KeepFinished, KEEP_FINISHED_KEY};
use notchling_locale::{self as t, Text};
use serde_json::Value;

pub const KEY: &str = KEEP_FINISHED_KEY;

pub const CHOICES: [(&str, Text); 3] = [
    ("1d", t::settings::KEEP_FINISHED_SESSIONS_V1D),
    ("7d", t::settings::KEEP_FINISHED_SESSIONS_V7D),
    ("30d", t::settings::KEEP_FINISHED_SESSIONS_V30D),
];

pub fn chosen(key: &str, value: &Value) -> Option<KeepFinished> {
    KeepFinished::chosen(key, value)
}

#[cfg(test)]
#[path = "keep_finished_tests.rs"]
mod tests;
