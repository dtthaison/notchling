use super::row::{ActivityRow, DbQuery};
use notchling_events::registry;
use notchling_events::spec::Lock;
use rusqlite::types::Value as Sql;
use rusqlite::{params, params_from_iter, Connection, OpenFlags, OptionalExtension};
use std::fmt;
use std::path::Path;
use std::time::Duration;

const SCHEMA: i64 = 1;
const BUSY: Duration = Duration::from_secs(2);
const TRIM_STEP: i64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbError(pub String);

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> DbError {
        DbError(e.to_string())
    }
}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> DbError {
        DbError(e.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    Existing,
    Created,
    Rebuilt,
}

pub struct EventsDb {
    db: Connection,
    opened: Opened,
}

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS activity (
  seq INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, kv INTEGER NOT NULL,
  grp TEXT NOT NULL, outcome TEXT NOT NULL, actor_tier TEXT, actor_via TEXT, actor_exe TEXT,
  actor_session TEXT, session TEXT, folder TEXT, account TEXT, device TEXT, op_id TEXT,
  host TEXT NOT NULL, audit_n INTEGER, spooled INTEGER NOT NULL DEFAULT 0,
  legacy INTEGER NOT NULL DEFAULT 0, f TEXT NOT NULL, expires INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS activity_ts ON activity(ts DESC);
CREATE INDEX IF NOT EXISTS activity_kind ON activity(kind, ts DESC);
CREATE INDEX IF NOT EXISTS activity_session ON activity(session, ts DESC);
CREATE INDEX IF NOT EXISTS activity_actor_session ON activity(actor_session, ts DESC);
CREATE INDEX IF NOT EXISTS activity_expires ON activity(expires);
CREATE TABLE IF NOT EXISTS metrics (start INTEGER NOT NULL, span TEXT NOT NULL, proc TEXT NOT NULL,
  name TEXT NOT NULL, labels TEXT NOT NULL, count INTEGER NOT NULL, sum INTEGER NOT NULL,
  buckets TEXT, PRIMARY KEY (start, span, proc, name, labels));
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const COLUMNS: &str = "seq, ts, kind, kv, grp, outcome, actor_tier, actor_via, actor_exe, \
actor_session, session, folder, account, device, op_id, host, audit_n, spooled, legacy, f, expires";

fn refuse_link(path: &Path) -> Result<(), DbError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(DbError(format!(
            "refusing to open a database through a symlink: {}",
            path.display()
        ))),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn private(path: &Path) -> Result<(), DbError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn private(_path: &Path) -> Result<(), DbError> {
    Ok(())
}

fn open_file(path: &Path) -> Result<(Connection, bool), DbError> {
    let fresh = !path.exists();
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = Connection::open_with_flags(path, flags)?;
    db.busy_timeout(BUSY)?;
    let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(DbError(format!("quick_check said {check}")));
    }
    let _ = db.pragma_update(None, "journal_mode", "WAL");
    db.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA {
        return Err(DbError("events.db is from a newer Notchling".into()));
    }
    db.execute_batch(SCHEMA_SQL)?;
    db.pragma_update(None, "user_version", SCHEMA)?;
    if fresh {
        private(path)?;
    }
    Ok((db, fresh))
}

fn set_aside(path: &Path) -> Result<(), DbError> {
    let aside = path.with_extension("db.damaged");
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(path, &aside)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        let _ = std::fs::remove_file(sidecar);
    }
    Ok(())
}

impl EventsDb {
    pub fn open(path: &Path) -> Result<EventsDb, DbError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            if !dir.is_dir() {
                notchling_platform::files::ensure_private_dir(dir)
                    .map_err(|e| DbError(e.to_string()))?;
            }
        }
        refuse_link(path)?;
        match open_file(path) {
            Ok((db, fresh)) => Ok(EventsDb {
                db,
                opened: if fresh {
                    Opened::Created
                } else {
                    Opened::Existing
                },
            }),
            Err(_) if path.is_file() => {
                set_aside(path)?;
                let (db, _) = open_file(path)?;
                Ok(EventsDb {
                    db,
                    opened: Opened::Rebuilt,
                })
            }
            Err(e) => Err(e),
        }
    }

    pub fn memory() -> Result<EventsDb, DbError> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA_SQL)?;
        Ok(EventsDb {
            db,
            opened: Opened::Created,
        })
    }

    pub fn opened(&self) -> Opened {
        self.opened
    }

    pub fn insert(&mut self, rows: &[ActivityRow]) -> Result<(), DbError> {
        if rows.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        {
            let mut stmt = tx.prepare_cached(&format!(
                "INSERT OR REPLACE INTO activity ({COLUMNS}) VALUES \
                 (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)"
            ))?;
            for row in rows {
                stmt.execute(params![
                    row.seq as i64,
                    row.ts,
                    row.kind,
                    row.kv,
                    row.grp,
                    row.outcome,
                    row.actor_tier,
                    row.actor_via,
                    row.actor_exe,
                    row.actor_session,
                    row.session,
                    row.folder,
                    row.account,
                    row.device,
                    row.op_id,
                    row.host,
                    row.audit_n.map(|n| n as i64),
                    row.spooled,
                    row.legacy,
                    row.f,
                    row.expires,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list(&self, q: &DbQuery) -> Result<Vec<ActivityRow>, DbError> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Sql> = Vec::new();
        if let Some(since) = q.since {
            args.push(Sql::Integer(since));
            clauses.push(format!("ts > ?{}", args.len()));
        }
        if let Some(before) = q.before_seq {
            args.push(Sql::Integer(before as i64));
            clauses.push(format!("seq < ?{}", args.len()));
        }
        if let Some(session) = &q.session {
            args.push(Sql::Text(session.clone()));
            clauses.push(format!("session = ?{}", args.len()));
        }
        for (column, values) in [("kind", &q.kinds), ("grp", &q.groups)] {
            if values.is_empty() {
                continue;
            }
            let mut marks = Vec::new();
            for value in values {
                args.push(Sql::Text(value.clone()));
                marks.push(format!("?{}", args.len()));
            }
            clauses.push(format!("{column} IN ({})", marks.join(",")));
        }
        let filter = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let limit = q.limit.clamp(1, 10_000);
        let sql = format!(
            "SELECT {COLUMNS} FROM activity {filter} ORDER BY ts DESC, seq DESC LIMIT {limit}"
        );
        let mut stmt = self.db.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), read_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn count(&self) -> u64 {
        self.db
            .query_row("SELECT COUNT(*) FROM activity", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
            .unwrap_or(0)
    }

    pub fn prune(&mut self, now_ms: i64) -> Result<u64, DbError> {
        let removed = self
            .db
            .execute("DELETE FROM activity WHERE expires < ?1", [now_ms])?;
        Ok(removed as u64)
    }

    pub fn expired_span(&self, now_ms: i64) -> Option<(i64, i64)> {
        self.db
            .query_row(
                "SELECT MIN(ts), MAX(ts) FROM activity WHERE expires < ?1",
                [now_ms],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .ok()
            .and_then(|(from, to)| Some((from?, to?)))
    }

    pub fn oldest_ts(&self) -> Option<i64> {
        self.db
            .query_row("SELECT MIN(ts) FROM activity", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .ok()
            .flatten()
    }

    pub fn size_bytes(&self) -> u64 {
        let pragma = |name: &str| -> i64 {
            self.db
                .pragma_query_value(None, name, |r| r.get(0))
                .unwrap_or(0)
        };
        let used = pragma("page_count") - pragma("freelist_count");
        (used.max(0) * pragma("page_size")) as u64
    }

    pub fn trim_to(&mut self, bytes: u64) -> Result<u64, DbError> {
        let mut removed = 0u64;
        while self.size_bytes() > bytes {
            let step = self.db.execute(
                "DELETE FROM activity WHERE seq IN \
                 (SELECT seq FROM activity ORDER BY ts ASC, seq ASC LIMIT ?1)",
                [TRIM_STEP],
            )?;
            if step == 0 {
                break;
            }
            removed += step as u64;
        }
        if removed > 0 {
            let _ = self.db.execute_batch("VACUUM");
        }
        Ok(removed)
    }

    pub fn clear(&mut self, groups: &[String], include_locked: bool) -> Result<u64, DbError> {
        if groups.is_empty() {
            return Ok(0);
        }
        let mut args: Vec<Sql> = Vec::new();
        let mut marks = Vec::new();
        for group in groups {
            args.push(Sql::Text(group.clone()));
            marks.push(format!("?{}", args.len()));
        }
        let mut sql = format!("DELETE FROM activity WHERE grp IN ({})", marks.join(","));
        if !include_locked {
            let mut locked = Vec::new();
            for spec in registry::all() {
                if matches!(spec.lock, Lock::Always(_)) {
                    args.push(Sql::Text(spec.name.to_owned()));
                    locked.push(format!("?{}", args.len()));
                }
            }
            if !locked.is_empty() {
                sql.push_str(&format!(" AND kind NOT IN ({})", locked.join(",")));
            }
        }
        Ok(self.db.execute(&sql, params_from_iter(args.iter()))? as u64)
    }

    pub fn meta_get(&self, key: &str) -> Option<String> {
        self.db
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn meta_set(&mut self, key: &str, value: &str) -> Result<(), DbError> {
        self.db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn max_seq(&self) -> u64 {
        self.db
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM activity", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .unwrap_or(0)
    }

    pub fn row_with_audit_n(&self, n: u64) -> Option<ActivityRow> {
        let sql = format!("SELECT {COLUMNS} FROM activity WHERE audit_n = ?1 LIMIT 1");
        self.db
            .query_row(&sql, [n as i64], read_row)
            .optional()
            .ok()
            .flatten()
    }
}

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityRow> {
    Ok(ActivityRow {
        seq: r.get::<_, i64>(0)? as u64,
        ts: r.get(1)?,
        kind: r.get(2)?,
        kv: r.get(3)?,
        grp: r.get(4)?,
        outcome: r.get(5)?,
        actor_tier: r.get(6)?,
        actor_via: r.get(7)?,
        actor_exe: r.get(8)?,
        actor_session: r.get(9)?,
        session: r.get(10)?,
        folder: r.get(11)?,
        account: r.get(12)?,
        device: r.get(13)?,
        op_id: r.get(14)?,
        host: r.get(15)?,
        audit_n: r.get::<_, Option<i64>>(16)?.map(|n| n as u64),
        spooled: r.get(17)?,
        legacy: r.get(18)?,
        f: r.get(19)?,
        expires: r.get(20)?,
    })
}
