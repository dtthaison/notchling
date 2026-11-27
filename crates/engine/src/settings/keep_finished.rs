use super::Settings;
use serde_json::Value;
use std::path::Path;

pub const KEEP_FINISHED_KEY: &str = "keepFinishedSessions";
pub const DAY_MS: i64 = 24 * 3600 * 1000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeepFinished {
    #[default]
    Days1,
    Days7,
    Days30,
}

impl KeepFinished {
    pub const ALL: [KeepFinished; 3] = [
        KeepFinished::Days1,
        KeepFinished::Days7,
        KeepFinished::Days30,
    ];

    pub fn parse(value: &str) -> Option<KeepFinished> {
        match value {
            "1d" => Some(KeepFinished::Days1),
            "7d" => Some(KeepFinished::Days7),
            "30d" => Some(KeepFinished::Days30),
            _ => None,
        }
    }

    pub fn from_setting(value: &str) -> KeepFinished {
        Self::parse(value).unwrap_or_default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            KeepFinished::Days1 => "1d",
            KeepFinished::Days7 => "7d",
            KeepFinished::Days30 => "30d",
        }
    }

    pub fn window_ms(self) -> i64 {
        match self {
            KeepFinished::Days1 => DAY_MS,
            KeepFinished::Days7 => 7 * DAY_MS,
            KeepFinished::Days30 => 30 * DAY_MS,
        }
    }

    pub fn from_settings(settings: &Settings) -> KeepFinished {
        settings
            .get_str(KEEP_FINISHED_KEY)
            .as_deref()
            .and_then(Self::parse)
            .unwrap_or_default()
    }

    pub fn read(data_dir: &Path) -> Option<KeepFinished> {
        Settings::open(&data_dir.join("settings.json"))
            .ok()
            .map(|s| Self::from_settings(&s))
    }

    pub fn window_ms_in(data_dir: &Path) -> i64 {
        Self::read(data_dir).unwrap_or_default().window_ms()
    }

    pub fn chosen(key: &str, value: &Value) -> Option<KeepFinished> {
        (key == KEEP_FINISHED_KEY)
            .then_some(value)?
            .as_str()
            .and_then(Self::parse)
    }
}

#[cfg(test)]
#[path = "keep_finished_tests.rs"]
mod tests;
