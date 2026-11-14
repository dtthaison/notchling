use crate::transcript::util::parse_date;
use serde_json::{json, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const REFERENCE_DATE: f64 = 978_307_200.0;

pub const FILE_NAME: &str = "account-log.json";

static WRITE: Mutex<()> = Mutex::new(());

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: f64,
    pub id: String,
}

pub fn load(file: &Path) -> Vec<Entry> {
    let Ok(data) = std::fs::read(file) else {
        return Vec::new();
    };
    parse(&data)
}

pub fn parse(data: &[u8]) -> Vec<Entry> {
    let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let at = match item.get("at")? {
                Value::Number(n) => n.as_f64()? + REFERENCE_DATE,
                Value::String(s) => parse_date(s)?,
                _ => return None,
            };
            Some(Entry { at, id })
        })
        .collect();
    entries.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

pub fn account_at(date: f64, entries: &[Entry]) -> String {
    entries
        .iter()
        .rev()
        .find(|e| e.at <= date)
        .map(|e| e.id.clone())
        .unwrap_or_default()
}

pub trait AccountSwitches: Send + Sync {
    fn switches(&self) -> Vec<Entry>;
}

pub struct NoSwitches;

impl AccountSwitches for NoSwitches {
    fn switches(&self) -> Vec<Entry> {
        Vec::new()
    }
}

pub fn merge(file: Vec<Entry>, recorded: Vec<Entry>) -> Vec<Entry> {
    let Some(first) = recorded.iter().map(|e| e.at).reduce(f64::min) else {
        return file;
    };
    let mut all: Vec<Entry> = file.into_iter().filter(|e| e.at < first).collect();
    all.extend(recorded);
    all.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    all
}

#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    Unchanged,
    Write(String),
    Unreadable,
}

pub fn plan_record(current: Option<&[u8]>, id: &str, now_ms: i64) -> Plan {
    let at = now_ms as f64 / 1000.0 - REFERENCE_DATE;
    let entry = json!({"at": at, "id": id}).to_string();
    let Some(bytes) = current.filter(|b| !b.iter().all(u8::is_ascii_whitespace)) else {
        return Plan::Write(format!("[{entry}]"));
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Plan::Unreadable;
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
        return Plan::Unreadable;
    };
    if parse(bytes).last().is_some_and(|last| last.id == id) {
        return Plan::Unchanged;
    }
    if items.is_empty() {
        return Plan::Write(format!("[{entry}]"));
    }
    let body = text.trim_end();
    let head = body[..body.len() - 1].trim_end();
    Plan::Write(format!("{head},{entry}]"))
}

pub fn record(file: &Path, id: &str, now_ms: i64) -> io::Result<bool> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let current = match std::fs::read(file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    match plan_record(current.as_deref(), id, now_ms) {
        Plan::Unchanged => Ok(false),
        Plan::Unreadable => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "account-log.json isn't a list of entries; left as it is",
        )),
        Plan::Write(text) => {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            notchling_platform::files::write_private_no_follow(file, text.as_bytes())
                .map_err(io::Error::other)?;
            Ok(true)
        }
    }
}
