use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

use super::query::filter::{Filter, SqlArg};
use super::query::fold;
use super::{IndexedSession, SessionMeta, RECENT_SECS};

const SCHEMA: i64 = 4;

pub(crate) const CORPUS_SESSIONS: &str = "sessions";

const WEIGHTS: (f64, f64, f64) = (10.0, 4.0, 1.0);

const RECENT_BONUS: f64 = 0.5;

pub fn match_expr<W: AsRef<str>>(words: &[W]) -> Option<String> {
    let terms: Vec<String> = words
        .iter()
        .map(AsRef::as_ref)
        .map(fold)
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| format!("\"{}\"*", w.trim().replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

pub fn score(bm25: f64, age_secs: f64) -> f64 {
    let fresh = (1.0 - age_secs.max(0.0) / RECENT_SECS).max(0.0);
    -bm25 * (1.0 + RECENT_BONUS * fresh)
}

#[derive(Clone, Debug, PartialEq)]
pub struct FtsDoc {
    pub key: String,
    pub title: String,
    pub folder: String,
    pub body: String,
    pub at: f64,
    pub facets: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocStamp {
    pub key: String,
    pub at: f64,
    pub facets: Vec<(String, String)>,
}

pub struct SearchDb {
    db: Mutex<Connection>,
}

impl SearchDb {
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
                 DROP TABLE IF EXISTS facets;
                 DROP TABLE IF EXISTS fts;
                 CREATE TABLE docs (
                     id INTEGER PRIMARY KEY,
                     key TEXT NOT NULL UNIQUE,
                     modified REAL NOT NULL,
                     meta TEXT NOT NULL,
                     body TEXT NOT NULL,
                     corpus TEXT NOT NULL DEFAULT 'sessions');
                 CREATE TABLE facets (
                     doc INTEGER NOT NULL,
                     name TEXT NOT NULL,
                     value TEXT NOT NULL,
                     PRIMARY KEY (name, value, doc)) WITHOUT ROWID;
                 CREATE VIRTUAL TABLE fts USING fts5(
                     title, folder, body, content = '', contentless_delete = 1,
                     tokenize = 'unicode61 remove_diacritics 2');",
            )
            .map_err(|e| e.to_string())?;
            db.pragma_update(None, "user_version", SCHEMA)
                .map_err(|e| e.to_string())?;
        }
        Ok(SearchDb { db: Mutex::new(db) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn sessions(&self) -> Vec<IndexedSession> {
        let mut out = Vec::new();
        let mut after = 0;
        loop {
            let (chunk, last) = self.session_chunk(after, 256);
            if chunk.is_empty() {
                return out;
            }
            after = last;
            out.extend(chunk);
        }
    }

    pub fn session_metas(&self) -> Vec<SessionMeta> {
        let db = self.lock();
        let Ok(mut query) = db.prepare("SELECT meta FROM docs WHERE corpus = ?1") else {
            return Vec::new();
        };
        let Ok(rows) = query.query_map([CORPUS_SESSIONS], |r| r.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten()
            .filter_map(|meta| serde_json::from_str(&meta).ok())
            .collect()
    }

    pub fn session_chunk(&self, after: i64, count: usize) -> (Vec<IndexedSession>, i64) {
        let db = self.lock();
        let Ok(mut query) = db.prepare(
            "SELECT id, meta, body FROM docs WHERE corpus = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
        ) else {
            return (Vec::new(), after);
        };
        let rows = query.query_map(params![CORPUS_SESSIONS, after, count as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        });
        let Ok(rows) = rows else {
            return (Vec::new(), after);
        };
        let mut last = after;
        let mut out = Vec::new();
        for (id, meta, body) in rows.flatten() {
            last = id;
            if let Ok(meta) = serde_json::from_str::<SessionMeta>(&meta) {
                out.push(IndexedSession::new(meta, body));
            }
        }
        (out, last)
    }

    pub fn clear_corpus(&self, corpus: &str) -> Result<(), String> {
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM facets WHERE doc IN (SELECT id FROM docs WHERE corpus = ?1)",
            [corpus],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM fts WHERE rowid IN (SELECT id FROM docs WHERE corpus = ?1)",
            [corpus],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM docs WHERE corpus = ?1", [corpus])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn put(&self, sessions: &[IndexedSession]) -> Result<(), String> {
        if sessions.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for s in sessions {
            let meta = serde_json::to_string(&s.meta).map_err(|e| e.to_string())?;
            let facets: Vec<(String, String)> = s
                .meta
                .facets()
                .into_iter()
                .map(|(n, v)| (n.to_owned(), v))
                .collect();
            put_row(
                &tx,
                CORPUS_SESSIONS,
                &Row {
                    key: &s.meta.key,
                    title: &s.meta.title,
                    folder: &s.meta.folder,
                    body: &s.text,
                    at: s.meta.modified,
                    meta: &meta,
                    facets: &facets,
                },
            )?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn put_docs(&self, corpus: &str, docs: &[FtsDoc]) -> Result<(), String> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for d in docs {
            let facets: serde_json::Map<String, serde_json::Value> = d
                .facets
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            let meta = serde_json::Value::Object(facets).to_string();
            put_row(
                &tx,
                corpus,
                &Row {
                    key: &d.key,
                    title: &d.title,
                    folder: &d.folder,
                    body: &d.body,
                    at: d.at,
                    meta: &meta,
                    facets: &d.facets,
                },
            )?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn remove_corpus_docs<K: AsRef<str>>(
        &self,
        corpus: &str,
        keys: &[K],
    ) -> Result<(), String> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for key in keys {
            tx.execute(
                "DELETE FROM facets WHERE doc =
                     (SELECT id FROM docs WHERE key = ?1 AND corpus = ?2)",
                params![key.as_ref(), corpus],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM fts WHERE rowid =
                     (SELECT id FROM docs WHERE key = ?1 AND corpus = ?2)",
                params![key.as_ref(), corpus],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM docs WHERE key = ?1 AND corpus = ?2",
                params![key.as_ref(), corpus],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn corpus_stamps(&self, corpus: &str) -> Vec<DocStamp> {
        let db = self.lock();
        let Ok(mut query) =
            db.prepare("SELECT key, modified, meta FROM docs WHERE corpus = ?1 ORDER BY key")
        else {
            return Vec::new();
        };
        let rows = query.query_map([corpus], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, String>(2)?,
            ))
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.flatten()
            .map(|(key, at, meta)| DocStamp {
                key,
                at,
                facets: facets_of(&meta),
            })
            .collect()
    }

    pub fn doc_body(&self, corpus: &str, key: &str) -> Option<String> {
        self.lock()
            .query_row(
                "SELECT body FROM docs WHERE key = ?1 AND corpus = ?2",
                params![key, corpus],
                |r| r.get(0),
            )
            .ok()
    }

    pub fn remove<K: AsRef<str>>(&self, keys: &[K]) -> Result<(), String> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut db = self.lock();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for key in keys {
            tx.execute(
                "DELETE FROM facets WHERE doc = (SELECT id FROM docs WHERE key = ?1)",
                [key.as_ref()],
            )
            .map_err(|e| e.to_string())?;
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

    pub fn rank<W: AsRef<str>>(&self, words: &[W], now: f64, limit: usize) -> Vec<(String, f64)> {
        self.rank_corpus(CORPUS_SESSIONS, words, now, limit)
    }

    pub fn rank_filtered<W: AsRef<str>>(
        &self,
        words: &[W],
        filter: &Filter,
        now: f64,
        limit: usize,
    ) -> Vec<(String, f64)> {
        self.rank_corpus_filtered(CORPUS_SESSIONS, words, filter, now, limit)
    }

    pub fn rank_corpus<W: AsRef<str>>(
        &self,
        corpus: &str,
        words: &[W],
        now: f64,
        limit: usize,
    ) -> Vec<(String, f64)> {
        self.rank_corpus_filtered(corpus, words, &Filter::default(), now, limit)
    }

    pub fn rank_corpus_filtered<W: AsRef<str>>(
        &self,
        corpus: &str,
        words: &[W],
        filter: &Filter,
        now: f64,
        limit: usize,
    ) -> Vec<(String, f64)> {
        let Some(expr) = match_expr(words) else {
            return Vec::new();
        };
        let (clause, args) = filter.sql("d.id", "d.modified");
        let db = self.lock();
        let sql = format!(
            "SELECT d.key, bm25(fts, {}, {}, {}), d.modified FROM fts
             JOIN docs d ON d.id = fts.rowid
             WHERE fts MATCH ?1 AND d.corpus = ?3{clause} ORDER BY 2 LIMIT ?2",
            WEIGHTS.0, WEIGHTS.1, WEIGHTS.2
        );
        let Ok(mut query) = db.prepare(&sql) else {
            return Vec::new();
        };
        let wide = (limit.saturating_mul(4)).max(limit) as i64;
        let mut bound: Vec<rusqlite::types::Value> =
            vec![expr.into(), wide.into(), corpus.to_owned().into()];
        bound.extend(args.into_iter().map(|a| match a {
            SqlArg::Text(t) => rusqlite::types::Value::Text(t),
            SqlArg::Real(r) => rusqlite::types::Value::Real(r),
        }));
        let rows = query.query_map(rusqlite::params_from_iter(bound), |r| {
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

    #[cfg(test)]
    pub(crate) fn hold(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.lock()
    }

    #[cfg(test)]
    pub(crate) fn facet_rows(&self) -> i64 {
        self.lock()
            .query_row("SELECT COUNT(*) FROM facets", [], |r| r.get(0))
            .unwrap_or(0)
    }
}

struct Row<'a> {
    key: &'a str,
    title: &'a str,
    folder: &'a str,
    body: &'a str,
    at: f64,
    meta: &'a str,
    facets: &'a [(String, String)],
}

fn put_row(tx: &rusqlite::Transaction<'_>, corpus: &str, row: &Row<'_>) -> Result<(), String> {
    tx.execute(
        "INSERT INTO docs (key, modified, meta, body, corpus) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(key) DO UPDATE SET modified = excluded.modified,
             meta = excluded.meta, body = excluded.body, corpus = excluded.corpus",
        params![row.key, row.at, row.meta, row.body, corpus],
    )
    .map_err(|e| e.to_string())?;
    let id: i64 = tx
        .query_row("SELECT id FROM docs WHERE key = ?1", [row.key], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM facets WHERE doc = ?1", [id])
        .map_err(|e| e.to_string())?;
    for (name, value) in row.facets {
        tx.execute(
            "INSERT OR IGNORE INTO facets (doc, name, value) VALUES (?1, ?2, ?3)",
            params![id, name, value],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute("DELETE FROM fts WHERE rowid = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO fts (rowid, title, folder, body) VALUES (?1, ?2, ?3, ?4)",
        params![id, fold(row.title), fold(row.folder), fold(row.body)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn facets_of(meta: &str) -> Vec<(String, String)> {
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(meta)
        .map(|map| {
            map.into_iter()
                .filter_map(|(k, v)| Some((k, v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
pub(super) fn private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
pub(super) fn private(_path: &Path) -> Result<(), String> {
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
                parent: None,
                account: None,
                started: None,
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

    fn doc(key: &str, title: &str, body: &str) -> FtsDoc {
        FtsDoc {
            key: key.into(),
            title: title.into(),
            folder: "notchling".into(),
            body: body.into(),
            at: NOW,
            facets: vec![("version".into(), "v1".into())],
        }
    }

    #[test]
    fn corpora_do_not_mix() {
        let db = SearchDb::memory().unwrap();
        db.put(&[session("claude:s", "talk", "/w", "a quokka session", 0.0)])
            .unwrap();
        db.put_docs(
            "pages",
            &[doc("page:a/index.html", "Report", "a quokka page")],
        )
        .unwrap();
        assert_eq!(
            keys(&db.rank_corpus("pages", &["quokka"], NOW, 10)),
            ["page:a/index.html"]
        );
        assert_eq!(keys(&db.rank(&["quokka"], NOW, 10)), ["claude:s"]);
        assert_eq!(db.sessions().len(), 1);
    }

    #[test]
    fn documents_keep_their_body_and_facets_and_can_be_removed() {
        let db = SearchDb::memory().unwrap();
        db.put_docs(
            "pages",
            &[doc("page:a", "One", "alpha"), doc("page:b", "Two", "beta")],
        )
        .unwrap();
        let stamps = db.corpus_stamps("pages");
        assert_eq!(stamps.len(), 2);
        assert_eq!(
            stamps[0].facets,
            [("version".to_string(), "v1".to_string())]
        );
        assert_eq!(db.doc_body("pages", "page:a").as_deref(), Some("alpha"));
        db.remove_corpus_docs("pages", &["page:a".to_string(), "claude:s".to_string()])
            .unwrap();
        assert_eq!(db.corpus_stamps("pages").len(), 1);
        assert!(db.rank_corpus("pages", &["alpha"], NOW, 10).is_empty());
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

    #[test]
    fn an_index_from_before_children_were_filed_under_a_parent_is_rebuilt() {
        let tmp = TempDir::new("fts-before-parent");
        let path = tmp.path("search.db");
        {
            let db = SearchDb::open(&path).unwrap();
            db.put(&[session("a", "t", "/w", "x", 0.0)]).unwrap();
            assert_eq!(db.sessions().len(), 1);
            db.lock().pragma_update(None, "user_version", 1).unwrap();
        }
        assert!(SearchDb::open(&path).unwrap().sessions().is_empty());
        assert_eq!(
            SearchDb::open(&path)
                .unwrap()
                .lock()
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            SCHEMA
        );
    }
}
