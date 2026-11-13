use std::path::Path;

use notchling_engine::settings::Settings;
use notchling_engine::transcript::tail::{set_chat_tail_bytes, CHAT_TAIL_BYTES};

pub const KEY: &str = "chatHistory";

pub const CHOICES: [(&str, &str); 4] = [
    ("2mb", "2 MB"),
    ("4mb", "4 MB"),
    ("8mb", "8 MB"),
    ("16mb", "16 MB"),
];

pub fn bytes(choice: &str) -> Option<u64> {
    match choice {
        "2mb" => Some(2_000_000),
        "4mb" => Some(4_000_000),
        "8mb" => Some(8_000_000),
        "16mb" => Some(16_000_000),
        _ => None,
    }
}

pub fn from_settings(settings: &Settings) -> u64 {
    settings
        .get_str(KEY)
        .as_deref()
        .and_then(bytes)
        .unwrap_or(CHAT_TAIL_BYTES)
}

pub fn read(data_dir: &Path) -> u64 {
    Settings::open(&data_dir.join("settings.json"))
        .ok()
        .map_or(CHAT_TAIL_BYTES, |s| from_settings(&s))
}

pub fn apply(data_dir: &Path) {
    set_chat_tail_bytes(read(data_dir));
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
