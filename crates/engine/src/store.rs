//! The app's own data in one SQLite database, `notchling.db` in the data folder (decision 0014):
//! settings and Chat's unsent replies today. Each value is its own row, so the app and the daemon
//! writing at once each keep their change (the whole-file JSON rewrite lost one of them), and a
//! crash mid-write leaves the last committed state.
//!
//! The file is 0600 in a 0700 folder Notchling makes (a folder already there is left alone), in WAL
//! mode with a busy timeout so concurrent writers wait instead of failing. A symlink where the
//! database belongs is refused: the file is Notchling's own, so a planted link must not steer writes.
//!
//! Files other programs read (`lanes.json`, `pages.json`, `machines.json`, the bridge's files,
//! Claude Code's `settings.json`) stay as they are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

/// The database's name in the data folder.
pub const DB_NAME: &str = "notchling.db";

/// The schema's version (`PRAGMA user_version`).
const SCHEMA: i64 = 1;

/// How long a write waits for another process's write to finish.
const BUSY: Duration = Duration::from_secs(5);

/// The database for `data_dir`.
pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_NAME)
}

/// Whether the database exists yet (opening for a read never makes one).
pub fn exists(data_dir: &Path) -> bool {
    db_path(data_dir).is_file()
}

/// Opens the database, making the folder and the file when they're missing.
pub fn open(data_dir: &Path) -> Result<Connection, String> {
    if !data_dir.is_dir() {
        notchling_platform::files::ensure_private_dir(data_dir).map_err(|e| e.to_string())?;
    }
    let path = db_path(data_dir);
    refuse_link(&path)?;
    let fresh = !path.exists();
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = Connection::open_with_flags(&path, flags).map_err(|e| e.to_string())?;
    if fresh {
        private(&path)?;
    }
    db.busy_timeout(BUSY).map_err(|e| e.to_string())?;
    db.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    db.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    migrate(&db)?;
    Ok(db)
}

/// Opens the database only when it exists: for reads, which never make one.
pub fn open_existing(data_dir: &Path) -> Result<Option<Connection>, String> {
    if !exists(data_dir) {
        refuse_link(&db_path(data_dir))?;
        return Ok(None);
    }
    open(data_dir).map(Some)
}

fn refuse_link(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "refusing to open a database through a symlink: {}",
            path.display()
        )),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn private(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn migrate(db: &Connection) -> Result<(), String> {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if version >= SCHEMA {
        return Ok(());
    }
    db.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS drafts (
             row_id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
         PRAGMA user_version = 1;
         COMMIT;",
    )
    .map_err(|e| e.to_string())
}

/// A `meta` value.
pub fn meta(db: &Connection, key: &str) -> Result<Option<String>, String> {
    db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

pub fn set_meta(db: &Connection, key: &str, value: &str) -> Result<(), String> {
    db.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Chat's unsent replies, by row id; none when there's no database yet.
pub fn drafts(data_dir: &Path) -> Result<HashMap<String, String>, String> {
    let Some(db) = open_existing(data_dir)? else {
        return Ok(HashMap::new());
    };
    let mut query = db
        .prepare("SELECT row_id, text FROM drafts WHERE text <> ''")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Puts one row's unsent reply (`""` forgets it).
pub fn put_draft(data_dir: &Path, row_id: &str, text: &str, now_ms: i64) -> Result<(), String> {
    if text.is_empty() && !exists(data_dir) {
        return Ok(());
    }
    let db = open(data_dir)?;
    let changed = if text.is_empty() {
        db.execute("DELETE FROM drafts WHERE row_id = ?1", [row_id])
    } else {
        db.execute(
            "INSERT INTO drafts (row_id, text, updated_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(row_id) DO UPDATE SET text = excluded.text, updated_ms = excluded.updated_ms",
            params![row_id, text, now_ms],
        )
    };
    changed.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    #[test]
    fn drafts_are_kept_one_row_each_and_reading_makes_nothing() {
        let tmp = TempDir::new("store-drafts");
        let dir = tmp.path("data");
        assert!(drafts(&dir).unwrap().is_empty());
        put_draft(&dir, "job-1", "", 1).unwrap();
        assert!(!dir.exists(), "reading and clearing make no database");
        put_draft(&dir, "job-1", "half a reply\nline two", 1).unwrap();
        put_draft(&dir, "codex:s1", "fix the build", 2).unwrap();
        put_draft(&dir, "job-1", "half a reply, longer", 3).unwrap();
        assert_eq!(
            drafts(&dir).unwrap(),
            HashMap::from([
                ("job-1".to_owned(), "half a reply, longer".to_owned()),
                ("codex:s1".to_owned(), "fix the build".to_owned()),
            ])
        );
        put_draft(&dir, "job-1", "", 4).unwrap();
        assert_eq!(drafts(&dir).unwrap().len(), 1);
    }

    #[test]
    fn two_writers_at_once_both_keep_their_rows() {
        let tmp = TempDir::new("store-writers");
        let dir = tmp.path("data");
        std::thread::scope(|scope| {
            for t in 0..6 {
                let dir = dir.clone();
                scope.spawn(move || {
                    for i in 0..30 {
                        put_draft(&dir, &format!("row-{t}"), &format!("text {i}"), i).unwrap();
                    }
                });
            }
        });
        let all = drafts(&dir).unwrap();
        assert_eq!(all.len(), 6);
        assert!(all.values().all(|t| t == "text 29"));
    }

    #[cfg(unix)]
    #[test]
    fn the_database_is_private_and_a_link_in_its_place_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("store-mode");
        let dir = tmp.path("data");
        put_draft(&dir, "a", "b", 1).unwrap();
        let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(db_path(&dir)), 0o600);
        assert_eq!(mode(dir.clone()), 0o700);

        let other = tmp.path("other");
        std::fs::create_dir_all(&other).unwrap();
        let victim = tmp.write("victim.db", "mine");
        std::os::unix::fs::symlink(&victim, db_path(&other)).unwrap();
        assert!(put_draft(&other, "a", "b", 1).is_err());
        assert!(drafts(&other).is_err());
        assert_eq!(std::fs::read_to_string(victim).unwrap(), "mine");
    }
}
