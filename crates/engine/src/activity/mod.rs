mod normalize;

pub use normalize::KINDS;

use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const DEFAULT_LIMIT: usize = 200;
pub const MAX_LIMIT: usize = 1000;
pub const MAX_BYTES: usize = 900 * 1024;
pub const AUDIT_TAIL: u64 = 2 * 1024 * 1024;
pub const PHONE_TAIL: u64 = 1024 * 1024;
pub const ACCOUNT_CAP: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub time_ms: u64,
    pub source: &'static str,
    pub kind: &'static str,
    pub op: String,
    pub who: String,
    pub result: String,
    pub detail: Option<String>,
}

impl Row {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "time_ms": self.time_ms,
            "source": self.source,
            "kind": self.kind,
            "op": self.op,
            "who": self.who,
            "result": self.result,
        });
        if let Some(detail) = &self.detail {
            v["detail"] = json!(detail);
        }
        v
    }
}

#[derive(Clone, Debug)]
pub struct Files {
    pub audit: PathBuf,
    pub remote_log: PathBuf,
    pub account_log: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub kinds: Vec<String>,
    pub since_ms: u64,
    pub limit: usize,
}

impl Query {
    pub fn from_args(args: &Value) -> Result<Query, String> {
        let mut kinds = Vec::new();
        match args.get("kinds") {
            None | Some(Value::Null) => {}
            Some(Value::Array(list)) => {
                for item in list {
                    let name = item.as_str().ok_or("kinds must be a list of names")?;
                    if !KINDS.contains(&name) {
                        return Err(format!("no activity kind named {name}"));
                    }
                    kinds.push(name.to_owned());
                }
            }
            Some(_) => return Err("kinds must be a list of names".into()),
        }
        let since_ms = match args.get("since") {
            None | Some(Value::Null) => 0,
            Some(v) => v.as_u64().ok_or("since must be a time in milliseconds")?,
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => DEFAULT_LIMIT,
            Some(v) => v.as_u64().ok_or("limit must be a number")? as usize,
        };
        Ok(Query {
            kinds,
            since_ms,
            limit: limit.clamp(1, MAX_LIMIT),
        })
    }

    fn keeps(&self, row: &Row) -> bool {
        row.time_ms > self.since_ms
            && (self.kinds.is_empty() || self.kinds.iter().any(|k| k == row.kind))
    }
}

fn old_copy(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

pub fn tail(path: &Path, max: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.take(max).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return text;
    }
    match text.split_once('\n') {
        Some((_, rest)) => rest.to_owned(),
        None => String::new(),
    }
}

fn whole(path: &Path, cap: u64) -> Vec<u8> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let _ = file.take(cap).read_to_end(&mut bytes);
    bytes
}

pub fn rows(files: &Files, query: &Query) -> Vec<Row> {
    let mut all: Vec<Row> = Vec::new();
    for path in [old_copy(&files.audit), files.audit.clone()] {
        all.extend(
            tail(&path, AUDIT_TAIL)
                .lines()
                .filter_map(normalize::audit_row),
        );
    }
    all.extend(
        tail(&files.remote_log, PHONE_TAIL)
            .lines()
            .filter_map(normalize::phone_row),
    );
    all.extend(normalize::account_rows(&whole(
        &files.account_log,
        ACCOUNT_CAP,
    )));
    all.retain(|row| query.keeps(row));
    all.sort_by_key(|row| std::cmp::Reverse(row.time_ms));
    all.truncate(query.limit);
    all
}

pub fn list(files: &Files, query: &Query) -> Value {
    let mut shown = rows(files, query);
    let mut truncated = false;
    loop {
        let body: Vec<Value> = shown.iter().map(Row::to_json).collect();
        let size = serde_json::to_vec(&body).map(|b| b.len()).unwrap_or(0);
        if size <= MAX_BYTES || shown.is_empty() {
            return json!({"rows": body, "truncated": truncated});
        }
        truncated = true;
        shown.truncate(shown.len() / 2);
    }
}

#[cfg(test)]
mod tests;
