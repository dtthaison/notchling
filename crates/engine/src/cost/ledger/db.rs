use super::{Call, LedgerError, Rollup};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use std::time::Duration;

const SCHEMA: i64 = 1;
const BUSY: Duration = Duration::from_secs(2);

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS calls (
  key INTEGER PRIMARY KEY, ts REAL NOT NULL, tool TEXT NOT NULL, session TEXT NOT NULL,
  agent TEXT NOT NULL, sub INTEGER NOT NULL, agent_type TEXT, model TEXT NOT NULL, effort TEXT,
  input INTEGER NOT NULL, output INTEGER NOT NULL, cache_read INTEGER NOT NULL,
  write_5m INTEGER NOT NULL, write_1h INTEGER NOT NULL, split INTEGER NOT NULL, cost REAL);
CREATE INDEX IF NOT EXISTS calls_ts ON calls(ts);
CREATE TABLE IF NOT EXISTS rollups (
  day TEXT NOT NULL, tool TEXT NOT NULL, sub INTEGER NOT NULL, model TEXT NOT NULL,
  calls INTEGER NOT NULL, input INTEGER NOT NULL, output INTEGER NOT NULL,
  cache_read INTEGER NOT NULL, write_5m INTEGER NOT NULL, write_1h INTEGER NOT NULL,
  cost REAL NOT NULL, unpriced INTEGER NOT NULL, PRIMARY KEY (day, tool, sub, model));
CREATE TABLE IF NOT EXISTS files (
  path TEXT PRIMARY KEY, size INTEGER NOT NULL, off INTEGER NOT NULL, state TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

#[cfg(unix)]
fn private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn private(_path: &Path) {}

fn start(db: &Connection) -> Result<(), LedgerError> {
    db.busy_timeout(BUSY)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA {
        return Err(LedgerError("usage.db is from a newer Notchling".into()));
    }
    db.execute_batch(SCHEMA_SQL)?;
    db.pragma_update(None, "user_version", SCHEMA)?;
    Ok(())
}

pub fn open_file(path: &Path) -> Result<Connection, LedgerError> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(LedgerError(format!(
                "refusing to open a database through a symlink: {}",
                path.display()
            )));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let fresh = !path.exists();
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = Connection::open_with_flags(path, flags)?;
    let _ = db.pragma_update(None, "journal_mode", "WAL");
    db.pragma_update(None, "synchronous", "NORMAL")?;
    start(&db)?;
    if fresh {
        private(path);
    }
    Ok(db)
}

pub fn open_memory() -> Result<Connection, LedgerError> {
    let db = Connection::open_in_memory()?;
    start(&db)?;
    Ok(db)
}

pub fn insert(db: &Connection, key: u64, call: &Call, day: &str) -> rusqlite::Result<bool> {
    let added = db.execute(
        "INSERT OR IGNORE INTO calls (key, ts, tool, session, agent, sub, agent_type, model, effort,
           input, output, cache_read, write_5m, write_1h, split, cost)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            key as i64,
            call.ts,
            call.tool,
            call.session,
            call.agent,
            call.sub,
            call.agent_type,
            call.model,
            call.effort,
            call.input as i64,
            call.output as i64,
            call.cache_read as i64,
            call.write_5m as i64,
            call.write_1h as i64,
            call.split,
            call.cost
        ],
    )?;
    if added == 0 {
        return Ok(false);
    }
    db.execute(
        "INSERT INTO rollups (day, tool, sub, model, calls, input, output, cache_read, write_5m,
           write_1h, cost, unpriced)
         VALUES (?1,?2,?3,?4,1,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(day, tool, sub, model) DO UPDATE SET
           calls = calls + 1, input = input + ?5, output = output + ?6,
           cache_read = cache_read + ?7, write_5m = write_5m + ?8, write_1h = write_1h + ?9,
           cost = cost + ?10, unpriced = unpriced + ?11",
        params![
            day,
            call.tool,
            call.sub,
            call.model,
            call.input as i64,
            call.output as i64,
            call.cache_read as i64,
            call.write_5m as i64,
            call.write_1h as i64,
            call.cost.unwrap_or(0.0),
            i64::from(call.cost.is_none())
        ],
    )?;
    Ok(true)
}

pub fn calls_since(db: &Connection, from: f64) -> Vec<Call> {
    let Ok(mut stmt) = db.prepare(
        "SELECT ts, tool, session, agent, sub, agent_type, model, effort, input, output,
           cache_read, write_5m, write_1h, split, cost
         FROM calls WHERE ts >= ?1 ORDER BY tool, session, agent, ts",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([from], |r| {
        Ok(Call {
            ts: r.get(0)?,
            tool: r.get(1)?,
            session: r.get(2)?,
            agent: r.get(3)?,
            sub: r.get(4)?,
            agent_type: r.get(5)?,
            model: r.get(6)?,
            effort: r.get(7)?,
            input: r.get::<_, i64>(8)? as u64,
            output: r.get::<_, i64>(9)? as u64,
            cache_read: r.get::<_, i64>(10)? as u64,
            write_5m: r.get::<_, i64>(11)? as u64,
            write_1h: r.get::<_, i64>(12)? as u64,
            split: r.get(13)?,
            cost: r.get(14)?,
        })
    });
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

pub fn rollups_since(db: &Connection, day: &str) -> Vec<Rollup> {
    let Ok(mut stmt) = db.prepare(
        "SELECT day, tool, sub, model, calls, input, output, cache_read, write_5m, write_1h, cost,
           unpriced FROM rollups WHERE day >= ?1 ORDER BY day, tool, sub, model",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map([day], |r| {
        Ok(Rollup {
            day: r.get(0)?,
            tool: r.get(1)?,
            sub: r.get(2)?,
            model: r.get(3)?,
            calls: r.get::<_, i64>(4)? as u64,
            input: r.get::<_, i64>(5)? as u64,
            output: r.get::<_, i64>(6)? as u64,
            cache_read: r.get::<_, i64>(7)? as u64,
            write_5m: r.get::<_, i64>(8)? as u64,
            write_1h: r.get::<_, i64>(9)? as u64,
            cost: r.get(10)?,
            unpriced: r.get::<_, i64>(11)? as u64,
        })
    });
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

pub fn call_count(db: &Connection) -> u64 {
    db.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get::<_, i64>(0))
        .map_or(0, |n| n as u64)
}

pub fn mark(db: &Connection, path: &str) -> Option<(u64, u64, String)> {
    db.query_row(
        "SELECT size, off, state FROM files WHERE path = ?1",
        [path],
        |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                r.get::<_, i64>(1)? as u64,
                r.get(2)?,
            ))
        },
    )
    .optional()
    .ok()
    .flatten()
}

pub fn set_mark(
    db: &Connection,
    path: &str,
    size: u64,
    offset: u64,
    state: &str,
) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO files (path, size, off, state) VALUES (?1,?2,?3,?4)
         ON CONFLICT(path) DO UPDATE SET size = ?2, off = ?3, state = ?4",
        params![path, size as i64, offset as i64, state],
    )?;
    Ok(())
}

pub fn marked_paths(db: &Connection) -> Vec<String> {
    let Ok(mut stmt) = db.prepare("SELECT path FROM files") else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |r| r.get(0));
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

pub fn forget(db: &Connection, path: &str) {
    let _ = db.execute("DELETE FROM files WHERE path = ?1", [path]);
}

pub fn meta_f64(db: &Connection, key: &str) -> Option<f64> {
    db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .ok()
    .flatten()
    .and_then(|v| v.parse().ok())
}

pub fn set_meta(db: &Connection, key: &str, value: &str) {
    let _ = db.execute(
        "INSERT INTO meta (key, value) VALUES (?1,?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    );
}

pub fn prune(db: &Connection, rows_before: f64, rollups_before: &str) {
    let _ = db.execute("DELETE FROM calls WHERE ts < ?1", [rows_before]);
    let _ = db.execute("DELETE FROM rollups WHERE day < ?1", [rollups_before]);
}
