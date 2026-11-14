use super::conversation::MAX_LINE;
use super::glance::has;
use super::scan::{scan_lines, Line};
use super::subagents::{notification_statuses, result_end};
use super::tail::RAW_BYTES;
use super::util::{parse_object, str_of};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

pub const WINDOW: u64 = 4_000_000;

type Finished = Arc<HashMap<String, String>>;
type Known = HashMap<String, (u64, Option<SystemTime>, Finished)>;

fn known() -> &'static Mutex<Known> {
    static CACHE: OnceLock<Mutex<Known>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn read(path: &str) -> Finished {
    let Ok(meta) = std::fs::metadata(path) else {
        return Finished::default();
    };
    let stamp = (meta.len(), meta.modified().ok());
    if let Some((len, modified, found)) =
        known().lock().unwrap_or_else(|e| e.into_inner()).get(path)
    {
        if (*len, *modified) == stamp {
            return found.clone();
        }
    }
    let found = Arc::new(scan(path, stamp.0));
    let mut cache = known().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 200 {
        cache.clear();
    }
    cache.insert(path.to_string(), (stamp.0, stamp.1, found.clone()));
    found
}

fn scan(path: &str, size: u64) -> HashMap<String, String> {
    let start = size.saturating_sub(WINDOW);
    let mut found = HashMap::new();
    let scanned = scan_lines(path, start, size, start > 0, MAX_LINE, |line| {
        if let Line::Whole(line) = line {
            note(line, &mut found);
        }
    });
    if let Ok(scanned) = scanned {
        RAW_BYTES.fetch_add(scanned.span, Ordering::Relaxed);
    }
    found
}

fn note(line: &[u8], into: &mut HashMap<String, String>) {
    let notified = has(line, b"<task-notification>");
    if !notified && !(has(line, b"\"agentId\"") && has(line, b"tool_result")) {
        return;
    }
    let Some(map) = parse_object(line) else {
        return;
    };
    let obj = Value::Object(map);
    if obj.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return;
    }
    if notified {
        if let Ok(text) = std::str::from_utf8(line) {
            for (id, status) in notification_statuses(&text.replace("\\/", "/")) {
                into.insert(id, status);
            }
        }
    }
    if str_of(&obj, "type") == Some("user") {
        results(&obj, into);
    }
}

fn results(obj: &Value, into: &mut HashMap<String, String>) {
    let flag = |key| obj.get(key).and_then(Value::as_bool) == Some(true);
    if flag("isMeta") || flag("isCompactSummary") {
        return;
    }
    let result = obj.get("toolUseResult").filter(|r| r.is_object());
    let Some(blocks) = obj
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for block in blocks
        .iter()
        .filter(|b| str_of(b, "type") == Some("tool_result"))
    {
        let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
        if let Some((ids, ended)) = result_end(str_of(block, "tool_use_id"), is_error, result) {
            for id in ids {
                into.insert(id.to_string(), ended.to_string());
            }
        }
    }
}
