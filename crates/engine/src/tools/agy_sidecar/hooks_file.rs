use super::{
    HOOK_NAME, INVOCATION_TIMEOUT_SECS, PRE_INVOCATION, PRE_TOOL, TOOL_MATCHER, TOOL_TIMEOUT_SECS,
};
use crate::accounts::json_splice::{remove_top_level, set_top_level};
use crate::install::file::{self, FileError, Rewritten, State};
use crate::install::{agy_hook_command, agy_hook_event};
use notchling_platform::paths::Os;
use serde_json::{json, Map, Value};
use std::path::Path;

const EVENTS: [&str; 2] = ["PreToolUse", "PreInvocation"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Install,
    Remove,
}

pub fn desired(binary: &Path, os: Os) -> Value {
    json!({
        "PreToolUse": [{
            "matcher": TOOL_MATCHER,
            "hooks": [{
                "type": "command",
                "command": agy_hook_command(binary, PRE_TOOL, os),
                "timeout": TOOL_TIMEOUT_SECS,
            }],
        }],
        "PreInvocation": [{
            "type": "command",
            "command": agy_hook_command(binary, PRE_INVOCATION, os),
            "timeout": INVOCATION_TIMEOUT_SECS,
        }],
    })
}

fn is_ours(handler: &Value) -> bool {
    handler
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| agy_hook_event(c).is_some())
}

fn count(value: &Value) -> (usize, usize) {
    let mut ours = 0;
    let mut others = 0;
    let Some(map) = value.as_object() else {
        return (0, 1);
    };
    for (event, list) in map {
        let Some(list) = list.as_array() else {
            continue;
        };
        for entry in list {
            let handlers: Vec<&Value> = match entry.get("hooks").and_then(Value::as_array) {
                Some(inner) => inner.iter().collect(),
                None => vec![entry],
            };
            for handler in handlers {
                if EVENTS.contains(&event.as_str()) && is_ours(handler) {
                    ours += 1;
                } else {
                    others += 1;
                }
            }
        }
    }
    (ours, others)
}

fn only_ours(value: &Value) -> bool {
    let (ours, others) = count(value);
    ours > 0 && others == 0
}

fn strip(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    let mut out = Map::new();
    let mut changed = false;
    for (key, list) in map {
        let Some(entries) = list.as_array().filter(|_| EVENTS.contains(&key.as_str())) else {
            out.insert(key.clone(), list.clone());
            continue;
        };
        let mut kept = Vec::new();
        for entry in entries {
            match entry.get("hooks").and_then(Value::as_array) {
                Some(inner) => {
                    let left: Vec<Value> = inner.iter().filter(|h| !is_ours(h)).cloned().collect();
                    if left.len() == inner.len() {
                        kept.push(entry.clone());
                        continue;
                    }
                    changed = true;
                    if !left.is_empty() {
                        let mut group = entry.clone();
                        group["hooks"] = Value::Array(left);
                        kept.push(group);
                    }
                }
                None if is_ours(entry) => changed = true,
                None => kept.push(entry.clone()),
            }
        }
        if !kept.is_empty() {
            out.insert(key.clone(), Value::Array(kept));
        } else if entries.is_empty() {
            out.insert(key.clone(), Value::Array(Vec::new()));
        }
    }
    changed.then_some(Value::Object(out))
}

fn has_events(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|m| m.keys().any(|k| k != "enabled"))
}

pub fn change(text: &str, binary: &Path, os: Os, want: Want) -> Result<Option<String>, String> {
    let blank = text.trim().is_empty();
    if blank {
        return match want {
            Want::Remove => Ok(None),
            Want::Install => set_top_level("{}", HOOK_NAME, &desired(binary, os))
                .map(|t| Some(format!("{t}\n")))
                .map_err(|e| e.to_string()),
        };
    }
    let root: Value = serde_json::from_str(text)
        .map_err(|e| format!("hooks.json is not valid JSON ({e}); it was left as it is"))?;
    let Some(map) = root.as_object() else {
        return Err("hooks.json is not a JSON object; it was left as it is".into());
    };
    let refuse = |e: crate::accounts::json_splice::SpliceError| {
        format!("hooks.json could not be edited safely ({e}); it was left as it is")
    };
    match want {
        Want::Install => {
            let wanted = desired(binary, os);
            if let Some(existing) = map.get(HOOK_NAME) {
                if !only_ours(existing) {
                    return Err(
                        "a hook named notchling is already in hooks.json and isn't Notchling's; it was left as it is"
                            .into(),
                    );
                }
                if *existing == wanted {
                    return Ok(None);
                }
            }
            set_top_level(text, HOOK_NAME, &wanted)
                .map(Some)
                .map_err(refuse)
        }
        Want::Remove => {
            let mut out = text.to_string();
            let mut changed = false;
            for (name, value) in map {
                let Some(stripped) = strip(value) else {
                    continue;
                };
                changed = true;
                out = if !has_events(&stripped) || (name == HOOK_NAME && only_ours(value)) {
                    remove_top_level(&out, name).map_err(refuse)?
                } else {
                    set_top_level(&out, name, &stripped).map_err(refuse)?
                };
            }
            Ok(changed.then_some(out))
        }
    }
}

pub fn apply(
    path: &Path,
    binary: &Path,
    os: Os,
    want: Want,
    stamp: u64,
) -> Result<Rewritten, FileError> {
    file::rewrite(path, stamp, &|text, state| {
        if state == State::Missing && want == Want::Remove {
            return Ok(None);
        }
        change(text, binary, os, want)
    })
}

pub fn check(path: &Path, binary: &Path, os: Os, want: Want) -> Result<(), FileError> {
    file::check_rewrite(path, &|text, state| {
        if state == State::Missing && want == Want::Remove {
            return Ok(None);
        }
        change(text, binary, os, want)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    Missing,
    Installed,
    Absent,
    Unreadable,
}

pub fn found(path: &Path) -> Found {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Found::Missing;
    };
    if text.trim().is_empty() {
        return Found::Absent;
    }
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else {
        return Found::Unreadable;
    };
    if map.values().any(|v| count(v).0 > 0) {
        Found::Installed
    } else {
        Found::Absent
    }
}
