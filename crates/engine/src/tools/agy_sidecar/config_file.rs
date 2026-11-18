use crate::accounts::json_splice::{remove_top_level, set_top_level};
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

type Parts = (Map<String, Value>, Map<String, Value>);

fn parts(text: &str, id: &str) -> Result<Parts, String> {
    if text.trim().is_empty() {
        return Err("Antigravity's config.json is empty; it was left as it is".into());
    }
    let root: Value = serde_json::from_str(text)
        .map_err(|e| format!("config.json is not valid JSON ({e}); it was left as it is"))?;
    let Some(map) = root.as_object() else {
        return Err("config.json is not a JSON object; it was left as it is".into());
    };
    let sidecars: Map<String, Value> = match map.get("sidecars") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Err(
                "config.json has a sidecars entry that isn't an object; it was left as it is"
                    .into(),
            )
        }
    };
    let entry: Map<String, Value> = match sidecars.get(id) {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Err(
                "config.json has a sidecar entry that isn't an object; it was left as it is".into(),
            )
        }
    };
    Ok((sidecars, entry))
}

fn turned_on(text: &str, id: &str) -> Result<Option<String>, String> {
    let (mut sidecars, mut entry) = parts(text, id)?;
    if entry.get("enabled") == Some(&json!(true)) {
        return Ok(None);
    }
    entry.insert("enabled".into(), json!(true));
    sidecars.insert(id.to_string(), Value::Object(entry));
    set_top_level(text, "sidecars", &Value::Object(sidecars))
        .map(Some)
        .map_err(|e| format!("config.json could not be edited safely ({e}); it was left as it is"))
}

fn turned_off(text: &str, id: &str) -> Result<Option<String>, String> {
    let (mut sidecars, mut entry) = parts(text, id)?;
    if entry.get("enabled") != Some(&json!(true)) {
        return Ok(None);
    }
    entry.remove("enabled");
    if entry.is_empty() {
        sidecars.remove(id);
    } else {
        sidecars.insert(id.to_string(), Value::Object(entry));
    }
    let edited = if sidecars.is_empty() {
        remove_top_level(text, "sidecars")
    } else {
        set_top_level(text, "sidecars", &Value::Object(sidecars))
    };
    edited
        .map(Some)
        .map_err(|e| format!("config.json could not be edited safely ({e}); it was left as it is"))
}

pub fn check_enable(path: &Path, id: &str) -> Result<(), EnableError> {
    if !path.is_file() {
        return Err(EnableError::NoConfig);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| EnableError::File(FileError::Io(format!("config.json: {e}"))))?;
    turned_on(&text, id)
        .map(|_| ())
        .map_err(|why| EnableError::File(FileError::Refused(why)))
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
    file::rewrite(path, stamp, &|text, _| turned_on(text, id)).map_err(EnableError::File)
}

pub fn disable(path: &Path, id: &str, stamp: u64) -> Result<Rewritten, FileError> {
    if !path.is_file() {
        return Ok(Rewritten {
            wrote: false,
            backup: None,
            state: file::State::Missing,
        });
    }
    file::rewrite(path, stamp, &|text, _| turned_off(text, id))
}
