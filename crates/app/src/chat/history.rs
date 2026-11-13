use std::path::Path;

use notchling_engine::settings::Settings;
use notchling_engine::transcript::tail::{chat_tail_bytes, set_chat_tail_bytes, CHAT_TAIL_BYTES};
use serde_json::Value;

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

pub fn read(data_dir: &Path) -> Option<u64> {
    Settings::open(&data_dir.join("settings.json"))
        .ok()
        .map(|s| from_settings(&s))
}

pub fn apply(data_dir: &Path) {
    if let Some(bytes) = read(data_dir) {
        set_chat_tail_bytes(bytes);
    }
}

pub fn chosen(key: &str, value: &Value) -> Option<u64> {
    (key == KEY).then_some(value)?.as_str().and_then(bytes)
}

pub fn on_change(key: &str, value: &Value) -> bool {
    let Some(bytes) = chosen(key, value) else {
        return false;
    };
    let changed = chat_tail_bytes() != bytes;
    set_chat_tail_bytes(bytes);
    changed
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
