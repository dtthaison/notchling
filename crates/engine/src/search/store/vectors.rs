use super::format::VecFormat;
use crate::search::corpus::Doc;
use crate::search::embed::ModelId;
use crate::search::fts::private;
use rusqlite::{params, Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

const SCHEMA: i64 = 1;
const CHECK_UP_TO: u64 = 64 * 1024 * 1024;

pub struct VectorDb {
    db: Mutex<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub corpus: String,
    pub docs: usize,
    pub chunks: usize,
    pub embedded: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub docs: usize,
    pub chunks: usize,
    pub facets: usize,
    pub vectors: usize,
}

fn kind_name(kind: crate::search::chunk::ChunkKind) -> &'static str {
    use crate::search::chunk::ChunkKind;
    match kind {
        ChunkKind::Head => "head",
        ChunkKind::Turn => "turn",
        ChunkKind::Page => "page",
        ChunkKind::Entry => "entry",
    }
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

fn file_len(path: &Path) -> u64 {
    std::fs::symlink_metadata(path).map_or(0, |m| m.len())
}

fn check_size(path: &Path) -> u64 {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    file_len(path).saturating_add(file_len(Path::new(&wal)))
}

fn check_db(db: &Connection) -> Result<(), String> {
    match db.query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0)) {
        Ok(line) if line == "ok" => Ok(()),
        Ok(line) => Err(format!("the vector database failed its check: {line}")),
        Err(e) => match &e {
            rusqlite::Error::SqliteFailure(inner, _)
                if matches!(
                    inner.code,
                    rusqlite::ffi::ErrorCode::DatabaseCorrupt
                        | rusqlite::ffi::ErrorCode::NotADatabase
                ) =>
            {
                Err(e.to_string())
            }
            _ => Ok(()),
        },
    }
}

impl VectorDb {
    pub fn open(path: &Path) -> Result<VectorDb, String> {
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

    fn open_file(path: &Path) -> Result<VectorDb, String> {
        let fresh = !path.exists();
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = Connection::open_with_flags(path, flags).map_err(err)?;
        if fresh {
            private(path)?;
        }
        let opened = Self::setup(db)?;
        if check_size(path) <= CHECK_UP_TO {
            opened.with(check_db)?;
        }
        Ok(opened)
    }

    pub fn memory() -> Result<VectorDb, String> {
        Self::setup(Connection::open_in_memory().map_err(err)?)
    }

    fn setup(db: Connection) -> Result<VectorDb, String> {
        db.busy_timeout(Duration::from_secs(5)).map_err(err)?;
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(err)?;
        if version != SCHEMA {
            db.execute_batch(
                "DROP TABLE IF EXISTS vectors;
                 DROP TABLE IF EXISTS chunks;
                 DROP TABLE IF EXISTS facets;
                 DROP TABLE IF EXISTS docs;
                 DROP TABLE IF EXISTS meta;
                 CREATE TABLE docs (id INTEGER PRIMARY KEY, corpus TEXT NOT NULL, key TEXT NOT NULL,
                     stamp TEXT NOT NULL, at REAL NOT NULL, title TEXT NOT NULL,
                     UNIQUE (corpus, key));
                 CREATE TABLE facets (doc INTEGER, name TEXT, value TEXT,
                     PRIMARY KEY (name, value, doc)) WITHOUT ROWID;
                 CREATE INDEX facets_doc ON facets (doc);
                 CREATE TABLE chunks (id INTEGER PRIMARY KEY, doc INTEGER, ord INTEGER, kind TEXT,
                     hash BLOB NOT NULL, text TEXT NOT NULL);
                 CREATE INDEX chunks_doc ON chunks (doc);
                 CREATE INDEX chunks_hash ON chunks (hash);
                 CREATE TABLE vectors (hash BLOB, model TEXT, vec BLOB,
                     PRIMARY KEY (hash, model)) WITHOUT ROWID;
                 CREATE TABLE meta (k TEXT PRIMARY KEY, v TEXT);",
            )
            .map_err(err)?;
            db.pragma_update(None, "user_version", SCHEMA)
                .map_err(err)?;
        }
        Ok(VectorDb { db: Mutex::new(db) })
    }

    pub(super) fn with<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        f(&self.db.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn stamps(&self, corpus: &str) -> HashMap<String, String> {
        self.with(|db| {
            let Ok(mut query) = db.prepare("SELECT key, stamp FROM docs WHERE corpus = ?1") else {
                return HashMap::new();
            };
            let Ok(rows) = query.query_map([corpus], |r| Ok((r.get(0)?, r.get(1)?))) else {
                return HashMap::new();
            };
            rows.flatten().collect()
        })
    }

    pub fn corpora(&self) -> Vec<String> {
        self.with(|db| {
            let Ok(mut query) = db.prepare("SELECT DISTINCT corpus FROM docs ORDER BY corpus")
            else {
                return Vec::new();
            };
            let Ok(rows) = query.query_map([], |r| r.get::<_, String>(0)) else {
                return Vec::new();
            };
            rows.flatten().collect()
        })
    }

    pub fn models(&self) -> Vec<ModelId> {
        self.with(|db| {
            let Ok(mut query) = db.prepare("SELECT DISTINCT model FROM vectors ORDER BY model")
            else {
                return Vec::new();
            };
            let Ok(rows) = query.query_map([], |r| r.get::<_, String>(0)) else {
                return Vec::new();
            };
            rows.flatten().map(ModelId).collect()
        })
    }

    pub fn put_doc(&self, corpus: &str, stamp: &str, doc: &Doc) -> Result<(), String> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction().map_err(err)?;
        let id: i64 = tx
            .query_row(
                "INSERT INTO docs (corpus, key, stamp, at, title) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (corpus, key) DO UPDATE SET
                     stamp = excluded.stamp, at = excluded.at, title = excluded.title
                 RETURNING id",
                params![corpus, doc.key, stamp, doc.at, doc.title],
                |r| r.get(0),
            )
            .map_err(err)?;
        tx.execute("DELETE FROM chunks WHERE doc = ?1", [id])
            .map_err(err)?;
        tx.execute("DELETE FROM facets WHERE doc = ?1", [id])
            .map_err(err)?;
        for (name, value) in &doc.facets {
            tx.execute(
                "INSERT OR IGNORE INTO facets (doc, name, value) VALUES (?1, ?2, ?3)",
                params![id, name, value],
            )
            .map_err(err)?;
        }
        for (ord, chunk) in doc.chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO chunks (doc, ord, kind, hash, text) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    ord as i64,
                    kind_name(chunk.kind),
                    crate::search::chunk::hash(&chunk.text).to_vec(),
                    chunk.text
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    pub fn remove_docs(&self, corpus: &str, keys: &[String]) -> Result<(), String> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction().map_err(err)?;
        for key in keys {
            let id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM docs WHERE corpus = ?1 AND key = ?2",
                    params![corpus, key],
                    |r| r.get(0),
                )
                .ok();
            let Some(id) = id else { continue };
            tx.execute("DELETE FROM chunks WHERE doc = ?1", [id])
                .map_err(err)?;
            tx.execute("DELETE FROM facets WHERE doc = ?1", [id])
                .map_err(err)?;
            tx.execute("DELETE FROM docs WHERE id = ?1", [id])
                .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    pub fn missing(&self, model: &ModelId, limit: usize) -> Vec<([u8; 16], String)> {
        self.with(|db| {
            let Ok(mut query) = db.prepare(
                "SELECT c.hash, c.text FROM chunks c
                 WHERE NOT EXISTS (SELECT 1 FROM vectors v WHERE v.hash = c.hash AND v.model = ?1)
                 GROUP BY c.hash ORDER BY MIN(c.id) LIMIT ?2",
            ) else {
                return Vec::new();
            };
            let limit = i64::try_from(limit).unwrap_or(i64::MAX);
            let Ok(rows) = query.query_map(params![model.0, limit], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
            }) else {
                return Vec::new();
            };
            rows.flatten()
                .filter_map(|(hash, text)| Some((<[u8; 16]>::try_from(hash).ok()?, text)))
                .collect()
        })
    }

    pub fn live(&self, hashes: &[[u8; 16]]) -> HashSet<[u8; 16]> {
        if hashes.is_empty() {
            return HashSet::new();
        }
        self.with(|db| {
            let Ok(mut query) = db.prepare("SELECT 1 FROM chunks WHERE hash = ?1 LIMIT 1") else {
                return HashSet::new();
            };
            let mut out = HashSet::new();
            for hash in hashes {
                if query.query_row(params![hash.to_vec()], |_| Ok(())).is_ok() {
                    out.insert(*hash);
                }
            }
            out
        })
    }

    pub fn put_vectors(
        &self,
        model: &ModelId,
        format: VecFormat,
        rows: &[([u8; 16], Vec<f32>)],
    ) -> Result<(), String> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO meta (k, v) VALUES (?1, ?2)
             ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            params![format!("format:{}", model.0), format.tag()],
        )
        .map_err(err)?;
        for (hash, vector) in rows {
            tx.execute(
                "INSERT OR REPLACE INTO vectors (hash, model, vec) VALUES (?1, ?2, ?3)",
                params![hash.to_vec(), model.0, format.encode(vector)],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    pub fn sweep(&self) -> Result<usize, String> {
        self.with(|db| {
            db.execute(
                "DELETE FROM vectors WHERE NOT EXISTS
                     (SELECT 1 FROM chunks c WHERE c.hash = vectors.hash)",
                [],
            )
            .map_err(err)
        })
    }

    pub fn drop_model(&self, model: &ModelId) -> Result<(), String> {
        self.with(|db| {
            db.execute("DELETE FROM vectors WHERE model = ?1", [&model.0])
                .map_err(err)?;
            db.execute(
                "DELETE FROM meta WHERE k = ?1",
                [format!("format:{}", model.0)],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn coverage(&self, model: &ModelId) -> Vec<Coverage> {
        self.with(|db| {
            let Ok(mut query) = db.prepare(
                "SELECT d.corpus, COUNT(DISTINCT d.id), COUNT(c.id),
                        COUNT(CASE WHEN EXISTS
                            (SELECT 1 FROM vectors v WHERE v.hash = c.hash AND v.model = ?1)
                            THEN 1 END)
                 FROM docs d LEFT JOIN chunks c ON c.doc = d.id GROUP BY d.corpus",
            ) else {
                return Vec::new();
            };
            let Ok(rows) = query.query_map([&model.0], |r| {
                Ok(Coverage {
                    corpus: r.get(0)?,
                    docs: r.get::<_, i64>(1)? as usize,
                    chunks: r.get::<_, i64>(2)? as usize,
                    embedded: r.get::<_, i64>(3)? as usize,
                })
            }) else {
                return Vec::new();
            };
            rows.flatten().collect()
        })
    }

    pub fn chunk_text(&self, hash: &[u8; 16]) -> Option<String> {
        self.with(|db| {
            db.query_row(
                "SELECT text FROM chunks WHERE hash = ?1 ORDER BY id LIMIT 1",
                params![hash.to_vec()],
                |r| r.get::<_, String>(0),
            )
            .ok()
        })
    }

    pub fn stats(&self) -> Stats {
        self.with(|db| {
            let count = |table: &str| -> usize {
                db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map(|n| n as usize)
                .unwrap_or(0)
            };
            Stats {
                docs: count("docs"),
                chunks: count("chunks"),
                facets: count("facets"),
                vectors: count("vectors"),
            }
        })
    }
}
