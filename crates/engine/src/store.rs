//! The app's own data in one SQLite database, `notchling.db` in the data folder (decision 0014):
//! Chat's unsent replies (`drafts`), settings (`settings`, a row per key) and every other file
//! Notchling alone used to rewrite whole (`docs`, a row per former file, changed in one transaction
//! so two writers never lose each other's change; [`import_file`] moves the old file in once).
//! Each value is its own row, so the app and the daemon writing at once each keep their change
//! (the whole-file JSON rewrite lost one of them), and a crash mid-write leaves the last committed
//! state.
//!
//! The file is 0600 in a 0700 folder Notchling makes (a folder already there is left alone), in WAL
//! mode with a busy timeout so concurrent writers wait instead of failing. A symlink where the
//! database belongs is refused: the file is Notchling's own, so a planted link must not steer writes.
//!
//! Files other programs read (`lanes.json`, `pages.json`, the Swift app's `machines.json` and
//! `vault.json`, the bridge's files, Claude Code's `settings.json`) stay as they are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

/// The database's name in the data folder.
pub const DB_NAME: &str = "notchling.db";

/// The schema's version (`PRAGMA user_version`).
const SCHEMA: i64 = 2;

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
    // Turning WAL on, and the first schema, take a lock SQLite's busy timeout doesn't wait on:
    // two processes opening a new database at once retry for a moment instead of failing.
    retry_busy(|| db.pragma_update(None, "journal_mode", "WAL"))?;
    db.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    migrate(&db)?;
    Ok(db)
}

/// Runs `step` again while another connection holds the lock, for up to [`BUSY`].
fn retry_busy<T>(mut step: impl FnMut() -> rusqlite::Result<T>) -> Result<T, String> {
    let until = std::time::Instant::now() + BUSY;
    loop {
        match step() {
            Ok(v) => return Ok(v),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if matches!(
                    e.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) && std::time::Instant::now() < until =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
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
    retry_busy(|| {
        db.execute_batch(
            "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS drafts (
             row_id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS docs (name TEXT PRIMARY KEY, value TEXT NOT NULL);
         PRAGMA user_version = 2;
         COMMIT;",
        )
    })
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

/// Every setting, key to the JSON text of its value; none when there's no database yet.
pub fn settings(data_dir: &Path) -> Result<HashMap<String, String>, String> {
    let Some(db) = open_existing(data_dir)? else {
        return Ok(HashMap::new());
    };
    let mut query = db
        .prepare("SELECT key, value FROM settings")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Sets one setting to `value` (JSON text), leaving every other row as it is.
pub fn put_setting(data_dir: &Path, key: &str, value: &str) -> Result<(), String> {
    update_settings(data_dir, None, &[(key.to_owned(), value.to_owned())], &[])
}

/// Forgets one setting; no database yet means nothing to forget.
pub fn remove_setting(data_dir: &Path, key: &str) -> Result<(), String> {
    update_settings(data_dir, None, &[], &[key.to_owned()])
}

/// Changes some settings in one transaction: first `seed` goes in, but only while there are no
/// settings at all (the old `settings.json`, moved in once; whoever gets there first wins), then
/// `puts`, then `removes`. Only the named rows are touched, so the app and the daemon changing
/// different keys at once both keep their change.
pub fn update_settings(
    data_dir: &Path,
    seed: Option<&[(String, String)]>,
    puts: &[(String, String)],
    removes: &[String],
) -> Result<(), String> {
    if seed.is_none() && puts.is_empty() && (removes.is_empty() || !exists(data_dir)) {
        return Ok(());
    }
    let mut db = open(data_dir)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let put = "INSERT INTO settings (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value";
    if let Some(seed) = seed {
        let any: bool = tx
            .query_row("SELECT EXISTS (SELECT 1 FROM settings)", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if !any {
            for (key, value) in seed {
                tx.execute(put, params![key, value])
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    for (key, value) in puts {
        tx.execute(put, params![key, value])
            .map_err(|e| e.to_string())?;
    }
    for key in removes {
        tx.execute("DELETE FROM settings WHERE key = ?1", [key])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// A named document (what used to be a whole JSON file); none when there's no database or row.
pub fn doc(data_dir: &Path, name: &str) -> Result<Option<String>, String> {
    let Some(db) = open_existing(data_dir)? else {
        return Ok(None);
    };
    db.query_row("SELECT value FROM docs WHERE name = ?1", [name], |r| {
        r.get(0)
    })
    .optional()
    .map_err(|e| e.to_string())
}

/// Replaces a named document.
pub fn put_doc(data_dir: &Path, name: &str, value: &str) -> Result<(), String> {
    update_doc(data_dir, name, |_| Ok(Some(value.to_owned()))).map(|_| ())
}

/// Reads a named document and writes what `change` makes of it in one transaction, so another
/// process changing it at the same moment waits instead of being overwritten. `change` returning
/// `None` removes the document; an error writes nothing. Returns what was written.
pub fn update_doc(
    data_dir: &Path,
    name: &str,
    change: impl FnOnce(Option<String>) -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    let mut db = open(data_dir)?;
    // IMMEDIATE takes the write lock before the read, waiting out another writer (busy timeout).
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let old: Option<String> = tx
        .query_row("SELECT value FROM docs WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let new = change(old)?;
    match &new {
        Some(v) => tx.execute(
            "INSERT INTO docs (name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value",
            params![name, v],
        ),
        None => tx.execute("DELETE FROM docs WHERE name = ?1", [name]),
    }
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(new)
}

/// Moves the file Notchling used before into the document `name`, once: a row already there wins,
/// and the file is removed after the row is committed. A link, a file that can't be read as text,
/// or one `valid` rejects is left where it is and nothing is written. Returns whether one moved in.
pub fn import_file(
    data_dir: &Path,
    name: &str,
    old: &Path,
    valid: impl Fn(&str) -> bool,
) -> Result<bool, String> {
    if std::fs::symlink_metadata(old).map_or(true, |m| !m.is_file()) {
        return Ok(false);
    }
    let Ok(text) = std::fs::read_to_string(old) else {
        return Ok(false);
    };
    if !valid(&text) {
        return Ok(false);
    }
    update_doc(data_dir, name, |row| Ok(Some(row.unwrap_or(text))))?;
    std::fs::remove_file(old).map_err(|e| e.to_string())?;
    Ok(true)
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
    fn settings_are_a_row_each_and_reading_or_forgetting_makes_nothing() {
        let tmp = TempDir::new("store-settings");
        let dir = tmp.path("data");
        assert!(settings(&dir).unwrap().is_empty());
        remove_setting(&dir, "showPet").unwrap();
        assert!(!dir.exists(), "reading and forgetting make no database");
        put_setting(&dir, "showPet", "false").unwrap();
        put_setting(&dir, "notchWidth", "\"large\"").unwrap();
        put_setting(&dir, "showPet", "true").unwrap();
        assert_eq!(
            settings(&dir).unwrap(),
            HashMap::from([
                ("showPet".to_owned(), "true".to_owned()),
                ("notchWidth".to_owned(), "\"large\"".to_owned()),
            ])
        );
        remove_setting(&dir, "showPet").unwrap();
        assert_eq!(settings(&dir).unwrap().len(), 1);
    }

    #[test]
    fn the_seed_goes_in_only_while_there_are_no_settings() {
        let tmp = TempDir::new("store-seed");
        let dir = tmp.path("data");
        let row = |k: &str, v: &str| (k.to_owned(), v.to_owned());
        let first = [row("a", "1"), row("b", "2")];
        update_settings(&dir, Some(&first), &[row("c", "3")], &["a".to_owned()]).unwrap();
        assert_eq!(
            settings(&dir).unwrap(),
            HashMap::from([row("b", "2"), row("c", "3")])
        );
        // Later, a seed that arrives second finds rows already there and adds nothing.
        update_settings(&dir, Some(&[row("z", "9")]), &[row("d", "4")], &[]).unwrap();
        assert_eq!(settings(&dir).unwrap().len(), 3);
        assert!(!settings(&dir).unwrap().contains_key("z"));
    }

    #[test]
    fn writers_of_different_settings_at_once_both_keep_theirs() {
        let tmp = TempDir::new("store-settings-writers");
        let dir = tmp.path("data");
        std::thread::scope(|scope| {
            for t in 0..6 {
                let dir = dir.clone();
                scope.spawn(move || {
                    for i in 0..10 {
                        put_setting(&dir, &format!("key{t}"), &i.to_string()).unwrap();
                    }
                });
            }
        });
        let all = settings(&dir).unwrap();
        assert_eq!(all.len(), 6);
        assert!(all.values().all(|v| v == "9"));
    }

    #[test]
    fn documents_change_in_one_transaction_and_reading_makes_nothing() {
        let tmp = TempDir::new("store-docs");
        let dir = tmp.path("data");
        assert_eq!(doc(&dir, "limits").unwrap(), None);
        assert!(!dir.exists(), "reading makes no database");
        put_doc(&dir, "limits", r#"{"a":1}"#).unwrap();
        assert_eq!(doc(&dir, "limits").unwrap().as_deref(), Some(r#"{"a":1}"#));
        assert!(update_doc(&dir, "limits", |_| Err("no".into())).is_err());
        assert_eq!(
            doc(&dir, "limits").unwrap().as_deref(),
            Some(r#"{"a":1}"#),
            "a failed change writes nothing"
        );
        update_doc(&dir, "limits", |_| Ok(None)).unwrap();
        assert_eq!(doc(&dir, "limits").unwrap(), None);
    }

    #[test]
    fn concurrent_updates_of_one_document_lose_nothing() {
        let tmp = TempDir::new("store-docs-writers");
        let dir = tmp.path("data");
        std::thread::scope(|scope| {
            for _ in 0..6 {
                let dir = dir.clone();
                scope.spawn(move || {
                    for _ in 0..10 {
                        update_doc(&dir, "n", |old| {
                            let n: i64 = old.as_deref().unwrap_or("0").parse().unwrap();
                            Ok(Some((n + 1).to_string()))
                        })
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(doc(&dir, "n").unwrap().as_deref(), Some("60"));
    }

    #[test]
    fn an_old_file_moves_in_once_and_a_bad_one_stays() {
        let tmp = TempDir::new("store-import");
        let dir = tmp.path("data");
        let old = tmp.write("data/limits.json", r#"{"v":1}"#);
        assert!(import_file(&dir, "limits", &old, |t| t.starts_with('{')).unwrap());
        assert!(!old.exists());
        assert_eq!(doc(&dir, "limits").unwrap().as_deref(), Some(r#"{"v":1}"#));

        let again = tmp.write("data/limits.json", r#"{"v":2}"#);
        assert!(import_file(&dir, "limits", &again, |_| true).unwrap());
        assert_eq!(
            doc(&dir, "limits").unwrap().as_deref(),
            Some(r#"{"v":1}"#),
            "the row already there wins"
        );

        let bad = tmp.write("data/other.json", "not json");
        assert!(!import_file(&dir, "other", &bad, |t| t.starts_with('{')).unwrap());
        assert!(bad.exists());
        assert_eq!(doc(&dir, "other").unwrap(), None);
        let missing = tmp.path("data/missing.json");
        assert!(!import_file(&dir, "none", &missing, |_| true).unwrap());
    }

    #[test]
    fn two_writers_at_once_both_keep_their_rows() {
        let tmp = TempDir::new("store-writers");
        let dir = tmp.path("data");
        std::thread::scope(|scope| {
            for t in 0..6 {
                let dir = dir.clone();
                scope.spawn(move || {
                    for i in 0..10 {
                        put_draft(&dir, &format!("row-{t}"), &format!("text {i}"), i).unwrap();
                    }
                });
            }
        });
        let all = drafts(&dir).unwrap();
        assert_eq!(all.len(), 6);
        assert!(all.values().all(|t| t == "text 9"));
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
