use std::path::Path;

use notchling_engine::settings::Settings;
use notchling_kit::{preference, set_preference, Preference};
use serde_json::Value;

pub const KEY: &str = "appearance";

pub const CHOICES: [(&str, notchling_locale::Text); 3] = [
    ("system", notchling_locale::settings::APPEARANCE_SYSTEM),
    ("light", notchling_locale::settings::APPEARANCE_LIGHT),
    ("dark", notchling_locale::settings::APPEARANCE_DARK),
];

pub fn from_settings(settings: &Settings) -> Preference {
    settings
        .get_str(KEY)
        .as_deref()
        .and_then(Preference::parse)
        .unwrap_or_default()
}

pub fn read(data_dir: &Path) -> Option<Preference> {
    Settings::open(&data_dir.join("settings.json"))
        .ok()
        .map(|s| from_settings(&s))
}

pub fn apply(chosen: Preference) {
    set_preference(chosen);
    #[cfg(target_os = "macos")]
    crate::mac::appearance::set_native(chosen);
}

pub fn chosen(key: &str, value: &Value) -> Option<Preference> {
    (key == KEY)
        .then_some(value)?
        .as_str()
        .and_then(Preference::parse)
}

pub fn changed_from(key: &str, value: &Value, current: Preference) -> Option<Preference> {
    chosen(key, value).filter(|p| *p != current)
}

pub fn changed_to(key: &str, value: &Value) -> Option<Preference> {
    changed_from(key, value, preference())
}

#[cfg(test)]
#[path = "appearance_tests.rs"]
mod tests;
