use super::manifest::Manifest;
use super::model::KEY_TOOLS;
use crate::catalog::Tool;
use crate::guard::{policy, DIR};
use crate::settings::Settings;
use std::path::{Path, PathBuf};

pub const KEY: &str = "costGuard";
pub const LOG: &str = "log";
pub const OFF: &str = "off";

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR)
}

pub fn level(data_dir: &Path) -> String {
    Settings::open(&data_dir.join("settings.json"))
        .ok()
        .and_then(|s| s.get_str(KEY))
        .filter(|l| l == LOG || l == OFF)
        .unwrap_or_else(|| LOG.to_string())
}

pub fn set_up(data_dir: &Path) -> bool {
    Manifest::load(data_dir)
        .map(|m| {
            m.entries
                .iter()
                .any(|e| e.tool == Tool::Claude && !e.removed)
        })
        .unwrap_or(false)
}

pub fn enabled(data_dir: &Path) -> bool {
    let claude_on = Settings::open(&data_dir.join("settings.json"))
        .ok()
        .and_then(|s| s.get_bool(KEY_TOOLS[0]))
        .unwrap_or(true);
    claude_on && level(data_dir) != OFF && set_up(data_dir)
}

pub fn sync(data_dir: &Path, cap: u64) -> bool {
    let on = enabled(data_dir);
    policy::sync(&dir(data_dir), on, cap);
    on
}
