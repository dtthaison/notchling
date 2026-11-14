use crate::bridge::foreign::ForeignRow;
use crate::sessions::iso::parse_iso_ms;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const RECENT_SECS: f64 = 14.0 * 86_400.0;

const LIMIT: usize = 200;

pub const DB_NAME: &str = "conversation_summaries.db";

const COPY_NAME: &str = "summaries.db";

static COPIES: OnceLock<Mutex<HashMap<PathBuf, Instant>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub modified: f64,
    pub running: bool,
}

impl Row {
    pub fn row_id(&self) -> String {
        format!("agy:{}", self.id)
    }

    pub fn to_foreign(&self, working: bool) -> ForeignRow {
        ForeignRow {
            id: self.row_id(),
            name: self.title.clone(),
            project: Path::new(&self.folder)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            cwd: self.folder.clone(),
            since_ms: self.modified * 1000.0,
            working,
            transcript_path: None,
        }
    }
}

pub fn rows(root: &Path, copy_dir: &Path, now_secs: f64) -> Vec<Row> {
    rows_cached(root, copy_dir, now_secs, Duration::ZERO)
}

pub fn rows_cached(root: &Path, copy_dir: &Path, now_secs: f64, reuse: Duration) -> Vec<Row> {
    with_copy(root, copy_dir, reuse, |copy| {
        read_copy(copy, now_secs).unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn children_cached(
    root: &Path,
    copy_dir: &Path,
    reuse: Duration,
    parent: &str,
) -> Vec<ChildRow> {
    with_copy(root, copy_dir, reuse, |copy| children(copy, parent)).unwrap_or_default()
}

pub fn copy_file(copy_dir: &Path) -> PathBuf {
    copy_dir.join(COPY_NAME)
}

fn with_copy<T>(
    root: &Path,
    copy_dir: &Path,
    reuse: Duration,
    read: impl FnOnce(&Path) -> T,
) -> Option<T> {
    let source = root.join(DB_NAME);
    if !source.is_file() {
        return None;
    }
    let mut made = COPIES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    notchling_platform::files::ensure_private_dir(copy_dir).ok()?;
    let copy = copy_file(copy_dir);
    let young = made
        .get(&copy)
        .is_some_and(|at| at.elapsed() < reuse && copy.is_file());
    if !young {
        for suffix in ["", "-wal", "-shm"] {
            let to = with_suffix(&copy, suffix);
            let _ = std::fs::remove_file(&to);
            let from = with_suffix(&source, suffix);
            if suffix != "-shm" && from.is_file() {
                let _ = std::fs::copy(&from, &to);
            }
        }
        made.insert(copy.clone(), Instant::now());
    }
    Some(read(&copy))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChildRow {
    pub id: String,
    pub agent_name: String,
    pub status: String,
    pub killed: bool,
    pub depth: u8,
    pub modified: f64,
}

pub fn children(copy: &Path, parent: &str) -> Vec<ChildRow> {
    read_children(copy, parent).unwrap_or_default()
}

fn read_children(copy: &Path, parent: &str) -> Result<Vec<ChildRow>, rusqlite::Error> {
    if !copy.is_file() {
        return Ok(Vec::new());
    }
    let db = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mut statement = db.prepare(
        "SELECT conversation_id, agent_name, status, killed, nesting_depth, last_modified_time \
         FROM conversation_summaries WHERE parent_conversation_id = ?1 \
         ORDER BY last_modified_time",
    )?;
    let mapped = statement.query_map([parent], |row| {
        let text = |i: usize| -> String {
            row.get::<_, Option<String>>(i)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        let killed = match row.get::<_, rusqlite::types::Value>(3)? {
            rusqlite::types::Value::Integer(n) => n != 0,
            rusqlite::types::Value::Text(t) => matches!(t.as_str(), "1" | "true" | "TRUE"),
            _ => false,
        };
        let depth = row.get::<_, Option<i64>>(4).ok().flatten().unwrap_or(1);
        Ok(ChildRow {
            id: text(0),
            agent_name: text(1),
            status: text(2),
            killed,
            depth: depth.clamp(1, 255) as u8,
            modified: date(&text(5)).unwrap_or(0.0),
        })
    })?;
    Ok(mapped.flatten().filter(|c| !c.id.is_empty()).collect())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn read_copy(copy: &Path, now_secs: f64) -> Result<Vec<Row>, rusqlite::Error> {
    let db = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mut statement = db.prepare(
        "SELECT conversation_id, title, preview, workspace_uris, status, last_modified_time \
         FROM conversation_summaries WHERE nesting_depth = 0 \
         ORDER BY last_modified_time DESC LIMIT ?1",
    )?;
    let text = |row: &rusqlite::Row<'_>, i: usize| -> String {
        row.get::<_, Option<String>>(i)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let mapped = statement.query_map([LIMIT as i64], |row| {
        Ok((
            text(row, 0),
            text(row, 1),
            text(row, 2),
            text(row, 3),
            text(row, 4),
            text(row, 5),
        ))
    })?;
    let mut out = Vec::new();
    for entry in mapped.flatten() {
        let (id, title, preview, uris, status, modified) = entry;
        let Some(modified) = date(&modified) else {
            continue;
        };
        if id.is_empty() || now_secs - modified >= RECENT_SECS {
            continue;
        }
        let title = [title, preview]
            .into_iter()
            .find(|t| !t.is_empty())
            .unwrap_or_else(|| "Antigravity Session".to_string());
        out.push(Row {
            id,
            title: title.chars().take(80).collect(),
            folder: folder(&uris),
            modified,
            running: status.contains("RUNNING"),
        });
    }
    Ok(out)
}

pub fn date(text: &str) -> Option<f64> {
    let head: String = text.chars().take(19).collect();
    let (day, clock) = head.split_once(' ')?;
    parse_iso_ms(&format!("{day}T{clock}Z")).map(|ms| ms as f64 / 1000.0)
}

pub fn folder(uris: &str) -> String {
    let Ok(list) = serde_json::from_str::<Vec<String>>(uris) else {
        return String::new();
    };
    list.first()
        .and_then(|u| u.strip_prefix("file://"))
        .map(percent_decode)
        .filter(|p| p.starts_with('/'))
        .unwrap_or_default()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn title(text: &str) -> String {
    let line = text.lines().next().unwrap_or(text);
    if line.chars().count() > 60 {
        let cut: String = line.chars().take(57).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
#[path = "summaries_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "summaries_children_tests.rs"]
mod children_tests;
