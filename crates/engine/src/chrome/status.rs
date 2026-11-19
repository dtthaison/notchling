use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub const ON: &str = "--chrome";
pub const OFF: &str = "--no-chrome";
pub const DEFAULT_KEY: &str = "claudeInChromeDefaultEnabled";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Flag,
    Default,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detected {
    pub on: Option<bool>,
    pub basis: Basis,
    pub default_enabled: Option<bool>,
}

pub fn flag_of(flags: &[String]) -> Option<bool> {
    flags.iter().rev().find_map(|f| match f.as_str() {
        ON => Some(true),
        OFF => Some(false),
        _ => None,
    })
}

pub fn with_flag(flags: &[String], on: bool) -> Vec<String> {
    let mut out: Vec<String> = flags
        .iter()
        .filter(|f| f.as_str() != ON && f.as_str() != OFF)
        .cloned()
        .collect();
    out.push(if on { ON } else { OFF }.to_string());
    out
}

pub fn default_enabled(claude_json: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(claude_json).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get(DEFAULT_KEY).and_then(Value::as_bool)
}

pub fn detect(flags: Option<&[String]>, default_enabled: Option<bool>) -> Detected {
    match flags.and_then(flag_of) {
        Some(on) => Detected {
            on: Some(on),
            basis: Basis::Flag,
            default_enabled,
        },
        None => match (flags, default_enabled) {
            (Some(_), Some(on)) => Detected {
                on: Some(on),
                basis: Basis::Default,
                default_enabled,
            },
            _ => Detected {
                on: None,
                basis: Basis::Unknown,
                default_enabled,
            },
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RewriteError {
    Read(String),
    NoFlags,
    Write(String),
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::Read(why) => write!(f, "Couldn't read the session's file: {why}"),
            RewriteError::NoFlags => write!(f, "The session's file lists no start flags."),
            RewriteError::Write(why) => write!(f, "Couldn't update the session's file: {why}"),
        }
    }
}

pub fn read_flags(state: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(state).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let list = value.get("respawnFlags")?.as_array()?;
    list.iter()
        .map(|f| f.as_str().map(str::to_string))
        .collect()
}

pub fn tasks_in_flight(state: &Path) -> u32 {
    let Ok(text) = std::fs::read_to_string(state) else {
        return 0;
    };
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    value
        .pointer("/inFlight/tasks")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

pub fn rewrite_flags(state: &Path, on: bool) -> Result<(), RewriteError> {
    let flags = read_flags(state).ok_or(RewriteError::NoFlags)?;
    set_flags(state, &with_flag(&flags, on))
}

pub fn set_flags(state: &Path, flags: &[String]) -> Result<(), RewriteError> {
    let text = std::fs::read_to_string(state).map_err(|e| RewriteError::Read(e.to_string()))?;
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| RewriteError::Read(e.to_string()))?;
    if value
        .get("respawnFlags")
        .and_then(Value::as_array)
        .is_none()
    {
        return Err(RewriteError::NoFlags);
    }
    value["respawnFlags"] = Value::from(flags.to_vec());
    let tmp = state.with_extension("json.notchling-tmp");
    let body = serde_json::to_vec(&value).map_err(|e| RewriteError::Write(e.to_string()))?;
    std::fs::write(&tmp, body).map_err(|e| RewriteError::Write(e.to_string()))?;
    if let Ok(meta) = std::fs::metadata(state) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, state).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        RewriteError::Write(e.to_string())
    })
}
