use super::summaries::{self, Row, DB_NAME, RECENT_SECS};
use crate::tools::agy_sidecar::ids::is_uuid;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DIR: &str = "antigravity";

const LIMIT: usize = 200;

const SUMMARY_FIELD: u64 = 17;
const SIDECAR_RECORD_FIELD: u64 = 12;
const SIDECAR_ENTRY_FIELD: u64 = 2;
const SIDECAR_ID_FIELD: u64 = 1;

pub fn root_beside(cli_root: &Path) -> PathBuf {
    cli_root.with_file_name(DIR)
}

pub fn copy_dir_in(copy_dir: &Path) -> PathBuf {
    copy_dir.join("app")
}

pub fn log_path(app_root: &Path, session: &str) -> PathBuf {
    super::elsewhere::log_path(app_root, session)
}

pub fn exists(app_root: &Path, session: &str) -> bool {
    is_uuid(session)
        && std::fs::symlink_metadata(app_root.join("conversations").join(format!("{session}.db")))
            .is_ok_and(|m| m.is_file())
}

pub fn ours(sidecar: &str) -> bool {
    sidecar == "notchling" || sidecar.starts_with("notchling/") || sidecar.starts_with("notchling-")
}

fn varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for (i, byte) in buf.iter().take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

fn length_delimited(buf: &[u8], wanted: u64) -> Option<&[u8]> {
    let mut at = 0;
    while at < buf.len() {
        let (key, used) = varint(&buf[at..])?;
        at += used;
        match key & 7 {
            0 => at += varint(buf.get(at..)?)?.1,
            1 => at += 8,
            2 => {
                let (len, used) = varint(buf.get(at..)?)?;
                at += used;
                let end = at.checked_add(usize::try_from(len).ok()?)?;
                let body = buf.get(at..end)?;
                if key >> 3 == wanted {
                    return Some(body);
                }
                at = end;
            }
            5 => at += 4,
            _ => return None,
        }
    }
    None
}

pub fn sidecar_of(raw_summary: &[u8]) -> Option<String> {
    let mut cursor = raw_summary;
    for field in [
        SUMMARY_FIELD,
        SIDECAR_RECORD_FIELD,
        SIDECAR_ENTRY_FIELD,
        SIDECAR_ID_FIELD,
    ] {
        cursor = length_delimited(cursor, field)?;
    }
    String::from_utf8(cursor.to_vec()).ok()
}

pub fn rows_cached(cli_root: &Path, copy_dir: &Path, now_secs: f64, reuse: Duration) -> Vec<Row> {
    let root = root_beside(cli_root);
    summaries::with_copy(&root, &copy_dir_in(copy_dir), reuse, |copy| {
        read_copy(copy, now_secs).unwrap_or_default()
    })
    .unwrap_or_default()
}

pub fn all_rows_cached(
    cli_root: &Path,
    copy_dir: &Path,
    now_secs: f64,
    reuse: Duration,
) -> Vec<Row> {
    let mut rows = summaries::rows_cached(cli_root, copy_dir, now_secs, reuse);
    for row in rows_cached(cli_root, copy_dir, now_secs, reuse) {
        if !rows.iter().any(|r| r.id == row.id) {
            rows.push(row);
        }
    }
    rows
}

pub fn watched_files(cli_root: &Path) -> [PathBuf; 2] {
    let db = root_beside(cli_root).join(DB_NAME);
    let mut wal = db.as_os_str().to_os_string();
    wal.push("-wal");
    [db, PathBuf::from(wal)]
}

fn read_copy(copy: &Path, now_secs: f64) -> Result<Vec<Row>, rusqlite::Error> {
    let db = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mut statement = db.prepare(
        "SELECT conversation_id, title, preview, workspace_uris, status, last_modified_time, \
         raw_summary FROM conversation_summaries \
         WHERE nesting_depth = 0 AND source NOT LIKE '%CLI%' \
         ORDER BY last_modified_time DESC LIMIT ?1",
    )?;
    let text = |row: &rusqlite::Row<'_>, i: usize| -> String {
        row.get::<_, Option<String>>(i)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let mapped = statement.query_map([LIMIT as i64], |row| {
        let raw: Option<Vec<u8>> = row.get(6).ok().flatten();
        Ok((
            text(row, 0),
            text(row, 1),
            text(row, 2),
            text(row, 3),
            text(row, 4),
            text(row, 5),
            raw.unwrap_or_default(),
        ))
    })?;
    let mut out = Vec::new();
    for (id, title, preview, uris, status, modified, raw) in mapped.flatten() {
        let Some(modified) = summaries::date(&modified) else {
            continue;
        };
        if !is_uuid(&id) || now_secs - modified >= RECENT_SECS {
            continue;
        }
        if !sidecar_of(&raw).is_some_and(|s| ours(&s)) {
            continue;
        }
        let title = [title, preview]
            .into_iter()
            .find(|t| !t.is_empty())
            .unwrap_or_else(|| "Antigravity Session".to_string());
        out.push(Row {
            id,
            title: title.chars().take(80).collect(),
            folder: summaries::folder(&uris),
            modified,
            running: status.contains("RUNNING"),
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "app_store_tests.rs"]
mod tests;
