use super::logic::{DEFAULT_SWITCH_AT, SWITCH_RANGE};
use crate::settings::spec;
use crate::store;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitMode {
    Off,
    Notify,
    NotifyAndSwitch,
}

impl LimitMode {
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
    modes: &ALL_MODES,
    default: LimitMode::Notify,
};
pub const PROVIDERS: [WatchedProvider; 3] = [CLAUDE, CODEX, ANTIGRAVITY];

const SWITCH_AT_KEY: &str = "limitSwitchAt";
const VERSION: u64 = 1;

pub const LIMITS_DOC: &str = "daemon/limits";

pub fn old_limits_file(data: &Path) -> PathBuf {
    data.join("daemon").join("limits.json")
}

pub struct LimitConfig {
    values: Mutex<Map<String, Value>>,
    data: Option<PathBuf>,
    corrupt: bool,
}

fn parse_values(text: &str) -> Option<Map<String, Value>> {
    let root = serde_json::from_str::<Value>(text).ok()?;
    let mut values = Map::new();
    if let Some(map) = root.get("values").and_then(Value::as_object) {
        for (k, v) in map {
            if let Some(s) = spec(k).filter(|s| s.accepts(v)) {
                values.insert(s.name.to_string(), v.clone());
            }
        }
    }
    Some(values)
}

impl LimitConfig {
    pub fn in_memory() -> LimitConfig {
        LimitConfig {
            values: Mutex::new(Map::new()),
            data: None,
            corrupt: false,
        }
    }

    pub fn open(data: &Path) -> LimitConfig {
        let old = old_limits_file(data);
        let _ = store::import_file(data, LIMITS_DOC, &old, |t| parse_values(t).is_some());
        let mut corrupt = std::fs::symlink_metadata(&old).is_ok();
        let mut values = Map::new();
        if let Ok(Some(text)) = store::doc(data, LIMITS_DOC) {
            match parse_values(&text) {
                Some(v) => values = v,
                None => corrupt = true,
            }
        }
        LimitConfig {
            values: Mutex::new(values),
            data: Some(data.to_path_buf()),
            corrupt,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Map<String, Value>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn mode(&self, provider: &WatchedProvider) -> LimitMode {
        self.lock()
            .get(provider.key)
            .and_then(Value::as_str)
            .and_then(LimitMode::parse)
            .filter(|m| provider.modes.contains(m))
            .unwrap_or(provider.default)
    }

    pub fn switch_at(&self) -> i64 {
        self.lock()
            .get(SWITCH_AT_KEY)
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_SWITCH_AT)
            .clamp(SWITCH_RANGE.0, SWITCH_RANGE.1)
    }

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
            return Err("the limits settings can't be read, so they aren't written over".into());
        }
        let Some(data) = &self.data else {
            values.insert(key.to_string(), value);
            return Ok(());
        };
        let written = store::update_doc(data, LIMITS_DOC, |old| {
            let mut stored = match old {
                Some(text) => parse_values(&text).ok_or("the limits settings can't be read")?,
                None => Map::new(),
            };
            stored.insert(key.to_string(), value);
            let root = json!({"version": VERSION, "values": stored});
            let mut text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
            text.push('\n');
            Ok(Some(text))
        })?;
        if let Some(stored) = written.as_deref().and_then(parse_values) {
            *values = stored;
        }
        Ok(())
    }
}
