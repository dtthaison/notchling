//! The session index on disk and its full-text search: one SQLite database (`search.db` in the
//! daemon's folder) with an FTS5 table over each session's title, folder and text, ranked by bm25
//! (title weighed most, then folder, then text) with a bonus for a recent change. It replaces the
//! JSON cache: the index loads its sessions from here at start and writes only the ones a refresh
//! read or dropped.
//!
//! Words match as prefixes of the text's words, case and accents folded (`unicode61
//! remove_diacritics 2`), so "sua" finds "sửa" and "rank" finds "ranking". A word inside another
//! ("ank") isn't a prefix; the index falls back to its substring match when bm25 finds nothing.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

use super::{IndexedSession, SessionMeta, RECENT_SECS};

/// The schema's version (`PRAGMA user_version`); another version is dropped and rebuilt.
const SCHEMA: i64 = 1;

/// Column weights for bm25: title, folder, text.
const WEIGHTS: (f64, f64, f64) = (10.0, 4.0, 1.0);

/// How much a session changed just now counts over one changed [`RECENT_SECS`] ago.
const RECENT_BONUS: f64 = 0.5;

/// The FTS5 query for `words`: each one quoted (its quotes doubled) and matched as a prefix, all of
/// them required. `None` when no word is left.
pub fn match_expr<W: AsRef<str>>(words: &[W]) -> Option<String> {
    let terms: Vec<String> = words
        .iter()
        .map(AsRef::as_ref)
        .map(str::trim)
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

/// The score a bm25 rank (lower is better) and an age give; higher is better.
pub fn score(bm25: f64, age_secs: f64) -> f64 {
    let fresh = (1.0 - age_secs.max(0.0) / RECENT_SECS).max(0.0);
    -bm25 * (1.0 + RECENT_BONUS * fresh)
}

/// The database. Shared by reference; one call at a time.
pub struct SearchDb {
    db: Mutex<Connection>,
}

impl SearchDb {
    /// Opens (or makes) the database at `path`: 0600, its folder 0700 when made here, a symlink in
    /// its place refused.
    pub fn open(path: &Path) -> Result<SearchDb, String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            if !dir.is_dir() {
                notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
            }
        }
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "refusing to open a database through a symlink: {}",
                path.display()
            ));
        }
        match Self::open_file(path) {
            Ok(db) => Ok(db),
            // Not a database (damaged, or something else wrote there): it's only an index, so it
            // is set aside and built again.
            Err(_) if path.is_file() => {
                let aside = path.with_extension("db.damaged");
                std::fs::rename(path, &aside).map_err(|e| e.to_string())?;
                Self::open_file(path)
            }
            Err(e) => Err(e),
        }
    }

    fn open_file(path: &Path) -> Result<SearchDb, String> {
        let fresh = !path.exists();
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
        if fresh {
            private(path)?;
        }
        Self::setup(db)
    }

    /// A database in memory only (tests, and an index with nowhere to keep it).
    pub fn memory() -> Result<SearchDb, String> {
        Self::setup(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn setup(db: Connection) -> Result<SearchDb, String> {
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != SCHEMA {
            db.execute_batch(
                "DROP TABLE IF EXISTS docs;
                 DROP TABLE IF EXISTS fts;
                 CREATE TABLE docs (
                     id INTEGER PRIMARY KEY,
                     key TEXT NOT NULL UNIQUE,
                     modified REAL NOT NULL,
                     meta TEXT NOT NULL);
                 CREATE VIRTUAL TABLE fts USING fts5(
                     title, folder, body, tokenize = 'unicode61 remove_diacritics 2');
                 PRAGMA user_version = 1;",
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(SearchDb { db: Mutex::new(db) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every session kept, as the index holds them.
    pub fn sessions(&self) -> Vec<IndexedSession> {
        let db = self.lock();
        let Ok(mut query) =
            db.prepare("SELECT d.meta, f.body FROM docs d JOIN fts f ON f.rowid = d.id")
        else {
            return Vec::new();
        };
        let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.flatten()
            .filter_map(|(meta, body)| {
                let meta: SessionMeta = serde_json::from_str(&meta).ok()?;
                Some(IndexedSession::new(meta, body))
            })
            .collect()
    }

    /// Adds or replaces `sessions`, by key, in one transaction.
    pub fn put(&self, sessions: &[IndexedSession]) -> Result<(), String> {
        if sessions.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for s in sessions {
            let meta = serde_json::to_string(&s.meta).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO docs (key, modified, meta) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET modified = excluded.modified, meta = excluded.meta",
                params![s.meta.key, s.meta.modified, meta],
            )
            .map_err(|e| e.to_string())?;
            let id: i64 = tx
                .query_row("SELECT id FROM docs WHERE key = ?1", [&s.meta.key], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM fts WHERE rowid = ?1", [id])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO fts (rowid, title, folder, body) VALUES (?1, ?2, ?3, ?4)",
                params![id, s.meta.title, s.meta.folder, s.text],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Forgets the sessions with these keys.
    pub fn remove<K: AsRef<str>>(&self, keys: &[K]) -> Result<(), String> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for key in keys {
            tx.execute(
                "DELETE FROM fts WHERE rowid = (SELECT id FROM docs WHERE key = ?1)",
                [key.as_ref()],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM docs WHERE key = ?1", [key.as_ref()])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// The best keys for `words` with their scores, best first, at most `limit`. `now` is seconds
    /// since 1970.
    pub fn rank<W: AsRef<str>>(&self, words: &[W], now: f64, limit: usize) -> Vec<(String, f64)> {
        let Some(expr) = match_expr(words) else {
            return Vec::new();
        };
        let db = self.lock();
        let sql = format!(
            "SELECT d.key, bm25(fts, {}, {}, {}), d.modified FROM fts
             JOIN docs d ON d.id = fts.rowid WHERE fts MATCH ?1 ORDER BY 2 LIMIT ?2",
            WEIGHTS.0, WEIGHTS.1, WEIGHTS.2
        );
        let Ok(mut query) = db.prepare(&sql) else {
            return Vec::new();
        };
        // More than asked, so the recency bonus can lift a slightly weaker match into the list.
        let wide = (limit.saturating_mul(4)).max(limit) as i64;
        let rows = query.query_map(params![expr, wide], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        let mut hits: Vec<(String, f64)> = rows
            .flatten()
            .map(|(key, bm25, modified)| (key, score(bm25, now - modified)))
            .collect();
        hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        hits
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    const NOW: f64 = 2_000_000_000.0;

    fn session(key: &str, title: &str, folder: &str, text: &str, age: f64) -> IndexedSession {
        IndexedSession::new(
            SessionMeta {
                key: key.into(),
                tool_key: "claude".into(),
                session_id: key.into(),
                title: title.into(),
                folder: folder.into(),
                path: String::new(),
                config_dir: None,
                modified: NOW - age,
                size: 0,
            },
            text.into(),
        )
    }

    fn keys(hits: &[(String, f64)]) -> Vec<&str> {
        hits.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn words_become_quoted_prefixes_all_required() {
        assert_eq!(
            match_expr(&["rank", "te\"st"]).as_deref(),
            Some("\"rank\"* AND \"te\"\"st\"*")
        );
        assert_eq!(match_expr(&["", "  ", "-"]), None);
        assert_eq!(match_expr::<&str>(&[]), None);
    }

    #[test]
    fn a_title_match_beats_the_same_words_in_the_text() {
        let db = SearchDb::memory().unwrap();
        db.put(&[
            session("a", "notes", "/w", "the ranking test fails on main", 60.0),
            session("b", "Port the ranking test", "/w", "work", 60.0),
            session("c", "other", "/w", "nothing here", 60.0),
        ])
        .unwrap();
        assert_eq!(keys(&db.rank(&["ranking", "test"], NOW, 10)), ["b", "a"]);
        assert_eq!(keys(&db.rank(&["rank"], NOW, 10)).len(), 2, "a prefix");
        assert!(db.rank(&["ank"], NOW, 10).is_empty(), "not a word inside");
    }

    #[test]
    fn accents_and_case_fold() {
        let db = SearchDb::memory().unwrap();
        db.put(&[session("v", "Sửa lỗi đăng nhập", "/w", "", 0.0)])
            .unwrap();
        assert_eq!(keys(&db.rank(&["sua", "LOI"], NOW, 10)), ["v"]);
    }

    #[test]
    fn of_two_equal_matches_the_recent_one_comes_first() {
        let db = SearchDb::memory().unwrap();
        db.put(&[
            session("old", "deploy", "/w", "", 13.0 * 86400.0),
            session("new", "deploy", "/w", "", 60.0),
        ])
        .unwrap();
        assert_eq!(keys(&db.rank(&["deploy"], NOW, 10)), ["new", "old"]);
        assert!(score(-2.0, 0.0) > score(-2.0, RECENT_SECS));
        assert_eq!(score(-2.0, RECENT_SECS), score(-2.0, 10.0 * RECENT_SECS));
    }

    #[test]
    fn put_replaces_remove_forgets_and_a_reopen_finds_them() {
        let tmp = TempDir::new("fts-reopen");
        let path = tmp.path("daemon/search.db");
        let db = SearchDb::open(&path).unwrap();
        db.put(&[session("a", "first title", "/w", "alpha", 0.0)])
            .unwrap();
        db.put(&[session("a", "second title", "/w", "beta", 0.0)])
            .unwrap();
        db.put(&[session("b", "keep", "/w", "gamma", 0.0)]).unwrap();
        assert!(db.rank(&["alpha"], NOW, 10).is_empty());
        assert_eq!(keys(&db.rank(&["beta"], NOW, 10)), ["a"]);
        db.remove(&["b"]).unwrap();
        drop(db);
        let again = SearchDb::open(&path).unwrap();
        let all = again.sessions();
        assert_eq!(all.len(), 1);
        assert_eq!(
            (all[0].meta.title.as_str(), all[0].text.as_str()),
            ("second title", "beta")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_and_a_link_in_its_place_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("fts-mode");
        let path = tmp.path("daemon/search.db");
        SearchDb::open(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let victim = tmp.write("victim.db", "mine");
        let link = tmp.path("other.db");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(SearchDb::open(&link).is_err());
        assert_eq!(std::fs::read_to_string(victim).unwrap(), "mine");
    }

    #[test]
    fn another_schema_version_is_rebuilt() {
        let tmp = TempDir::new("fts-version");
        let path = tmp.path("search.db");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE docs (x); PRAGMA user_version = 7;")
                .unwrap();
        }
        let db = SearchDb::open(&path).unwrap();
        db.put(&[session("a", "t", "/w", "x", 0.0)]).unwrap();
        assert_eq!(db.sessions().len(), 1);
    }
}
