//! What to do when an account nears a usage limit, per provider, and where the choice is kept.
//!
//! The keys are the Swift app's (`limitMode`, `limitSwitchAt`, `codexLimitMode`,
//! `antigravityLimitMode`, in [`crate::settings::KEYS`]) with the same defaults: Claude Code
//! Notify and Switch, Codex Notify, Antigravity Notify (Off or Notify only: a switch restarts the
//! app, and its limits exist only while it runs). The daemon keeps its own copy of these four
//! values in a file of its own; it never writes the Swift app's `UserDefaults`.

use super::logic::{DEFAULT_SWITCH_AT, SWITCH_RANGE};
use crate::settings::spec;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What to do when the account in use nears a usage limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitMode {
    Off,
    Notify,
    NotifyAndSwitch,
}

impl LimitMode {
    /// The stored name (`LimitMode.rawValue`).
    pub fn as_str(self) -> &'static str {
        match self {
            LimitMode::Off => "off",
            LimitMode::Notify => "notify",
            LimitMode::NotifyAndSwitch => "notifyAndSwitch",
        }
    }

    pub fn parse(text: &str) -> Option<LimitMode> {
        [
            LimitMode::Off,
            LimitMode::Notify,
            LimitMode::NotifyAndSwitch,
        ]
        .into_iter()
        .find(|m| m.as_str() == text)
    }

    pub fn title(self) -> &'static str {
        match self {
            LimitMode::Off => "Off",
            LimitMode::Notify => "Notify",
            LimitMode::NotifyAndSwitch => "Notify and Switch",
        }
    }
}

pub const ALL_MODES: [LimitMode; 3] = [
    LimitMode::Off,
    LimitMode::Notify,
    LimitMode::NotifyAndSwitch,
];
pub const NO_SWITCH_MODES: [LimitMode; 2] = [LimitMode::Off, LimitMode::Notify];

/// A provider whose limits are watched: its setting key, the modes it allows and its default.
#[derive(Clone, Copy, Debug)]
pub struct WatchedProvider {
    pub id: &'static str,
    pub label: &'static str,
    pub key: &'static str,
    pub modes: &'static [LimitMode],
    pub default: LimitMode,
}

pub const CLAUDE: WatchedProvider = WatchedProvider {
    id: "claude",
    label: "Claude Code",
    key: "limitMode",
    modes: &ALL_MODES,
    default: LimitMode::NotifyAndSwitch,
};
pub const CODEX: WatchedProvider = WatchedProvider {
    id: "codex",
    label: "Codex",
    key: "codexLimitMode",
    modes: &ALL_MODES,
    default: LimitMode::Notify,
};
pub const ANTIGRAVITY: WatchedProvider = WatchedProvider {
    id: "antigravity",
    label: "Antigravity",
    key: "antigravityLimitMode",
    modes: &NO_SWITCH_MODES,
    default: LimitMode::Notify,
};
pub const PROVIDERS: [WatchedProvider; 3] = [CLAUDE, CODEX, ANTIGRAVITY];

const SWITCH_AT_KEY: &str = "limitSwitchAt";
const VERSION: u64 = 1;

/// The four values, in memory and (when given a file) on disk.
pub struct LimitConfig {
    values: Mutex<Map<String, Value>>,
    file: Option<PathBuf>,
    /// The file exists and can't be read: kept as it is, never written over.
    corrupt: bool,
}

impl LimitConfig {
    /// Defaults only; nothing is written (tests, and a daemon given a test socket).
    pub fn in_memory() -> LimitConfig {
        LimitConfig {
            values: Mutex::new(Map::new()),
            file: None,
            corrupt: false,
        }
    }

    /// Reads `file` (`{"version": 1, "values": {...}}`). A missing file is the defaults; an
    /// unreadable one reads as the defaults too and is never written over.
    pub fn open(file: &Path) -> LimitConfig {
        let mut values = Map::new();
        let mut corrupt = false;
        match std::fs::read(file) {
            Ok(data) => match serde_json::from_slice::<Value>(&data) {
                Ok(root) => {
                    if let Some(map) = root.get("values").and_then(Value::as_object) {
                        for (k, v) in map {
                            if let Some(s) = spec(k).filter(|s| s.accepts(v)) {
                                values.insert(s.name.to_string(), v.clone());
                            }
                        }
                    }
                }
                Err(_) => corrupt = true,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => corrupt = true,
        }
        LimitConfig {
            values: Mutex::new(values),
            file: Some(file.to_path_buf()),
            corrupt,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Map<String, Value>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The mode for `provider`: the stored one when this provider allows it, else its default.
    pub fn mode(&self, provider: &WatchedProvider) -> LimitMode {
        self.lock()
            .get(provider.key)
            .and_then(Value::as_str)
            .and_then(LimitMode::parse)
            .filter(|m| provider.modes.contains(m))
            .unwrap_or(provider.default)
    }

    /// The point a switch happens at, held to 80 to 99 as a read (`LimitWatcher.switchAt`).
    pub fn switch_at(&self) -> i64 {
        self.lock()
            .get(SWITCH_AT_KEY)
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_SWITCH_AT)
            .clamp(SWITCH_RANGE.0, SWITCH_RANGE.1)
    }

    /// Sets a provider's mode; refused when the provider doesn't allow it.
    pub fn set_mode(&self, provider: &WatchedProvider, mode: LimitMode) -> Result<(), String> {
        if !provider.modes.contains(&mode) {
            return Err(format!(
                "{} can't be set to {}",
                provider.label,
                mode.title()
            ));
        }
        self.put(provider.key, json!(mode.as_str()))
    }

    /// Sets the switch point; refused outside 80 to 99.
    pub fn set_switch_at(&self, percent: i64) -> Result<(), String> {
        if !(SWITCH_RANGE.0..=SWITCH_RANGE.1).contains(&percent) {
            return Err(format!(
                "the switch point is {} to {}",
                SWITCH_RANGE.0, SWITCH_RANGE.1
            ));
        }
        self.put(SWITCH_AT_KEY, json!(percent))
    }

    fn put(&self, key: &str, value: Value) -> Result<(), String> {
        let mut values = self.lock();
        if self.corrupt {
            return Err("the limits settings file can't be read, so it isn't written over".into());
        }
        let before = values.insert(key.to_string(), value);
        if let Err(e) = self.save(&values) {
            match before {
                Some(old) => values.insert(key.to_string(), old),
                None => values.remove(key),
            };
            return Err(e);
        }
        Ok(())
    }

    fn save(&self, values: &Map<String, Value>) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
        }
        let root = json!({"version": VERSION, "values": values});
        let mut text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        text.push('\n');
        notchling_platform::files::write_private_no_follow(file, text.as_bytes())
            .map_err(|e| e.to_string())
    }
}
