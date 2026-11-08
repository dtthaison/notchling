//! Antigravity CLI conversations for the session list, from `conversation_summaries.db` in
//! `~/.gemini/antigravity-cli` (Swift `AntigravitySessions`).
//!
//! The database is in WAL mode and a read-only open fails while no WAL file exists, so it is read
//! from a copy (database and WAL) in a folder of ours; Antigravity's own file is never opened.

use crate::bridge::foreign::ForeignRow;
use crate::sessions::iso::parse_iso_ms;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Rows older than this are left out (14 days).
pub const RECENT_SECS: f64 = 14.0 * 86_400.0;

/// The most rows read.
const LIMIT: usize = 200;

/// The database's name in Antigravity's folder.
pub const DB_NAME: &str = "conversation_summaries.db";

/// The copy's name in ours.
const COPY_NAME: &str = "summaries.db";

static COPYING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub folder: String,
    /// Seconds since the epoch.
    pub modified: f64,
    pub running: bool,
}

impl Row {
    /// Chat's row id.
    pub fn row_id(&self) -> String {
        format!("agy:{}", self.id)
    }

    /// As the bridge and the notch list it.
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

/// The conversations of the last 14 days, newest first, read through a copy of `root`'s database
/// made in `copy_dir`. Empty when there is no database or the copy can't be read.
pub fn rows(root: &Path, copy_dir: &Path, now_secs: f64) -> Vec<Row> {
    let source = root.join(DB_NAME);
    if !source.is_file() {
        return Vec::new();
    }
    let _one = COPYING.lock().unwrap_or_else(|e| e.into_inner());
    if notchling_platform::files::ensure_private_dir(copy_dir).is_err() {
        return Vec::new();
    }
    let copy = copy_dir.join(COPY_NAME);
    for suffix in ["", "-wal", "-shm"] {
        let to = with_suffix(&copy, suffix);
        let _ = std::fs::remove_file(&to);
        let from = with_suffix(&source, suffix);
        if suffix != "-shm" && from.is_file() {
            let _ = std::fs::copy(&from, &to);
        }
    }
    read_copy(&copy, now_secs).unwrap_or_default()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Opened read-write so SQLite replays the copied WAL into the copy.
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

/// `2026-11-05 16:57:03.335872+00:00` (UTC) as seconds since the epoch; only the first 19
/// characters count, as in the Swift reader.
pub fn date(text: &str) -> Option<f64> {
    let head: String = text.chars().take(19).collect();
    let (day, clock) = head.split_once(' ')?;
    parse_iso_ms(&format!("{day}T{clock}Z")).map(|ms| ms as f64 / 1000.0)
}

/// The first of `["file:///path", ...]`, or empty.
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

/// A title from the first message, until Antigravity names the session.
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
