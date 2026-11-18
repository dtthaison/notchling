use crate::accounts::json_splice::set_top_level;
use crate::install::file::{self, FileError, Rewritten};
use serde_json::{json, Map, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    Unset,
}

#[derive(Debug)]
pub enum EnableError {
    NotConfirmed,
    NoConfig,
    File(FileError),
}

impl std::fmt::Display for EnableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnableError::NotConfirmed => write!(f, "Turning the sidecar on needs your confirmation."),
            EnableError::NoConfig => write!(
                f,
                "Antigravity has not made its config.json yet. Open the Antigravity app once, then try again."
            ),
            EnableError::File(e) => write!(f, "{e}"),
        }
    }
}

pub fn switch(path: &Path, id: &str) -> Result<Switch, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(Switch::Unset);
    };
    if text.trim().is_empty() {
        return Ok(Switch::Unset);
    }
    let root: Value = serde_json::from_str(&text)
        .map_err(|_| "Antigravity's config.json can't be read".to_string())?;
    Ok(
        match root
            .pointer(&format!("/sidecars/{}/enabled", id.replace('/', "~1")))
            .and_then(Value::as_bool)
        {
            Some(true) => Switch::On,
            Some(false) => Switch::Off,
            None => Switch::Unset,
        },
    )
}

pub fn enable(
    path: &Path,
    id: &str,
    confirmed: bool,
    stamp: u64,
) -> Result<Rewritten, EnableError> {
    if !confirmed {
        return Err(EnableError::NotConfirmed);
    }
    if !path.is_file() {
        return Err(EnableError::NoConfig);
    }
    file::rewrite(path, stamp, &|text, _| {
        if text.trim().is_empty() {
            return Err("Antigravity's config.json is empty; it was left as it is".into());
        }
        let root: Value = serde_json::from_str(text)
            .map_err(|e| format!("config.json is not valid JSON ({e}); it was left as it is"))?;
        let Some(map) = root.as_object() else {
            return Err("config.json is not a JSON object; it was left as it is".into());
        };
        let mut sidecars: Map<String, Value> =
            match map.get("sidecars") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(m)) => m.clone(),
                Some(_) => return Err(
                    "config.json has a sidecars entry that isn't an object; it was left as it is"
                        .into(),
                ),
            };
        let mut entry: Map<String, Value> =
            match sidecars.get(id) {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(m)) => m.clone(),
                Some(_) => return Err(
                    "config.json has a sidecar entry that isn't an object; it was left as it is"
                        .into(),
                ),
            };
        if entry.get("enabled") == Some(&json!(true)) {
            return Ok(None);
        }
        entry.insert("enabled".into(), json!(true));
        sidecars.insert(id.to_string(), Value::Object(entry));
        set_top_level(text, "sidecars", &Value::Object(sidecars))
            .map(Some)
            .map_err(|e| {
                format!("config.json could not be edited safely ({e}); it was left as it is")
            })
    })
    .map_err(EnableError::File)
}
