use super::rules::{Limits, BUILDER_CAP};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

pub const FILE: &str = "policy.json";
pub const MAX_FILE: u64 = 16 * 1024;
pub const LEVEL_LOG: &str = "log";

pub fn read(dir: &Path) -> Option<Limits> {
    let file = std::fs::File::open(dir.join(FILE)).ok()?;
    let mut text = String::new();
    file.take(MAX_FILE).read_to_string(&mut text).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("level").and_then(Value::as_str) != Some(LEVEL_LOG) {
        return None;
    }
    let cap = value
        .get("cap")
        .and_then(Value::as_u64)
        .filter(|n| (1..=64).contains(n))
        .unwrap_or(BUILDER_CAP);
    Some(Limits {
        cap,
        ..Limits::default()
    })
}

pub fn write(dir: &Path, cap: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = json!({"v": 1, "level": LEVEL_LOG, "cap": cap}).to_string();
    let tmp = dir.join("policy.json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(FILE))
}

pub fn clear(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

pub fn sync(dir: &Path, on: bool, cap: u64) {
    if on {
        let _ = write(dir, cap);
    } else {
        clear(dir);
    }
}
