use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanKind {
    Agent,
    Shell,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanEntry {
    pub id: String,
    pub kind: FanKind,
    pub label: String,
    pub started: Option<f64>,
}

pub fn parse(bytes: &[u8]) -> Vec<FanEntry> {
    let Ok(state) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let Some(fan) = state.get("fan").and_then(Value::as_array) else {
        return Vec::new();
    };
    fan.iter().filter_map(entry).collect()
}

fn entry(item: &Value) -> Option<FanEntry> {
    if item.get("doneAt").is_some_and(|done| !done.is_null()) {
        return None;
    }
    let kind = match item.get("kind").and_then(Value::as_str)? {
        "agent" => FanKind::Agent,
        "shell" => FanKind::Shell,
        _ => return None,
    };
    let id = item.get("id").and_then(Value::as_str)?;
    if id.is_empty() {
        return None;
    }
    Some(FanEntry {
        id: id.to_owned(),
        kind,
        label: item
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        started: item
            .get("startedAt")
            .and_then(Value::as_f64)
            .map(|ms| ms / 1000.0),
    })
}

pub fn state_path(config_dir: &Path, short: &str) -> Option<PathBuf> {
    let plain = !short.is_empty()
        && short
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| config_dir.join("jobs").join(short).join("state.json"))
}

#[derive(Default)]
pub struct FanReader {
    seen: HashMap<PathBuf, (u64, SystemTime, Arc<Vec<FanEntry>>)>,
}

impl FanReader {
    pub fn read(&mut self, path: &Path) -> Arc<Vec<FanEntry>> {
        let Some(stamp) = std::fs::metadata(path)
            .ok()
            .and_then(|m| Some((m.len(), m.modified().ok()?)))
        else {
            self.seen.remove(path);
            return Arc::default();
        };
        if let Some((len, modified, fan)) = self.seen.get(path) {
            if (*len, *modified) == stamp {
                return fan.clone();
            }
        }
        let fan = Arc::new(
            std::fs::read(path)
                .map(|bytes| parse(&bytes))
                .unwrap_or_default(),
        );
        self.seen
            .insert(path.to_path_buf(), (stamp.0, stamp.1, fan.clone()));
        fan
    }
}

#[cfg(test)]
#[path = "job_fan_tests.rs"]
mod tests;
