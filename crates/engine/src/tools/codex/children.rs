use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;

use super::list::days_from_civil;
use crate::transcript::codex::lineage_of_payload;
use crate::transcript::tail::RAW_BYTES;
use crate::transcript::util::{date_field, parse_object, str_of};

pub const HEAD_BYTES: u64 = 65_536;
pub const BACKFILL_DAYS: u32 = 30;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rollouts (\
    path TEXT PRIMARY KEY, size INTEGER, mtime REAL, thread TEXT, parent TEXT, \
    depth INTEGER, nickname TEXT, role TEXT, started REAL);\
    CREATE INDEX IF NOT EXISTS rollouts_parent ON rollouts(parent);";

#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub path: PathBuf,
    pub thread: String,
    pub parent: Option<String>,
    pub depth: u8,
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub started: f64,
}

pub fn head(path: &Path) -> Option<Head> {
    let mut line = Vec::new();
    let mut reader = BufReader::new(File::open(path).ok()?.take(HEAD_BYTES));
    reader.read_until(b'\n', &mut line).ok()?;
    RAW_BYTES.fetch_add(line.len() as u64, Ordering::Relaxed);
    if line.last() != Some(&b'\n') && line.len() as u64 >= HEAD_BYTES {
        return None;
    }
    let obj = Value::Object(parse_object(&line)?);
    if str_of(&obj, "type") != Some("session_meta") {
        return None;
    }
    let payload = obj.get("payload").filter(|p| p.is_object())?;
    let thread = str_of(payload, "id").filter(|s| !s.is_empty())?.to_owned();
    let lineage = lineage_of_payload(payload);
    Some(Head {
        path: path.to_path_buf(),
        thread,
        depth: lineage.as_ref().map_or(0, |l| l.depth),
        nickname: lineage.as_ref().and_then(|l| l.nickname.clone()),
        role: lineage.as_ref().and_then(|l| l.role.clone()),
        parent: lineage.map(|l| l.parent),
        started: date_field(&obj).unwrap_or(0.0),
    })
}

pub struct Index {
    db: Mutex<Connection>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn secs(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Head> {
    Ok(Head {
        path: PathBuf::from(r.get::<_, String>(0)?),
        thread: r.get(1)?,
        parent: r.get(2)?,
        depth: u8::try_from(r.get::<_, i64>(3)?).unwrap_or(u8::MAX),
        nickname: r.get(4)?,
        role: r.get(5)?,
        started: r.get(6)?,
    })
}

impl Index {
    pub fn open(path: &Path) -> Result<Index, String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let db = Connection::open_with_flags(path, flags).map_err(|e| e.to_string())?;
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        Index::setup(db)
    }

    pub fn open_in_memory() -> Result<Index, String> {
        Index::setup(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn setup(db: Connection) -> Result<Index, String> {
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Index { db: Mutex::new(db) })
    }

    pub fn stamp(&self, path: &Path) -> Option<(u64, f64)> {
        lock(&self.db)
            .query_row(
                "SELECT size, mtime FROM rollouts WHERE path = ?1",
                [path.to_string_lossy()],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, f64>(1)?)),
            )
            .ok()
    }

    pub fn note(&self, head: &Head, size: u64, mtime: f64) {
        self.note_many(&[(head.clone(), size, mtime)]);
    }

    pub fn note_many(&self, heads: &[(Head, u64, f64)]) {
        let mut db = lock(&self.db);
        let Ok(tx) = db.transaction() else {
            return;
        };
        for (head, size, mtime) in heads {
            let _ = tx.execute(
                "INSERT OR REPLACE INTO rollouts \
                 (path, size, mtime, thread, parent, depth, nickname, role, started) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    head.path.to_string_lossy(),
                    *size as i64,
                    mtime,
                    head.thread,
                    head.parent,
                    i64::from(head.depth),
                    head.nickname,
                    head.role,
                    head.started
                ],
            );
        }
        let _ = tx.commit();
    }

    pub fn children(&self, thread: &str) -> Vec<Head> {
        if thread.is_empty() {
            return Vec::new();
        }
        let db = lock(&self.db);
        let Ok(mut statement) = db.prepare(
            "SELECT path, thread, parent, depth, nickname, role, started \
             FROM rollouts WHERE parent = ?1 ORDER BY started, path",
        ) else {
            return Vec::new();
        };
        statement
            .query_map([thread], row)
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn is_child(&self, thread: &str) -> bool {
        lock(&self.db)
            .query_row(
                "SELECT 1 FROM rollouts WHERE thread = ?1 AND parent IS NOT NULL LIMIT 1",
                [thread],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn sweep(&self, day: &Path, present: &[PathBuf]) -> usize {
        let mut db = lock(&self.db);
        let prefix = format!("{}/", day.to_string_lossy());
        let known: Vec<String> = {
            let Ok(mut statement) =
                db.prepare("SELECT path FROM rollouts WHERE substr(path, 1, ?2) = ?1")
            else {
                return 0;
            };
            statement
                .query_map(params![prefix, prefix.chars().count() as i64], |r| {
                    r.get::<_, String>(0)
                })
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
        };
        let present: HashSet<&Path> = present.iter().map(PathBuf::as_path).collect();
        let gone: Vec<&String> = known
            .iter()
            .filter(|p| {
                let path = Path::new(p.as_str());
                path.parent() == Some(day) && !present.contains(path)
            })
            .collect();
        let Ok(tx) = db.transaction() else {
            return 0;
        };
        for path in &gone {
            let _ = tx.execute("DELETE FROM rollouts WHERE path = ?1", [path.as_str()]);
        }
        let _ = tx.commit();
        gone.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Refreshed {
    pub read: usize,
    pub kept: usize,
}

pub fn refresh(index: &Index, paths: &[PathBuf]) -> Refreshed {
    let mut out = Refreshed::default();
    let mut found = Vec::new();
    for path in paths {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        let size = meta.len();
        let mtime = meta.modified().map(secs).unwrap_or(0.0);
        if index.stamp(path) == Some((size, mtime)) {
            out.kept += 1;
            continue;
        }
        out.read += 1;
        if let Some(head) = head(path) {
            found.push((head, size, mtime));
        }
    }
    index.note_many(&found);
    out
}

pub fn rollout_files(day: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(day) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("rollout-") && name.ends_with(".jsonl")
        })
        .collect();
    files.sort();
    files
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn day_folder(sessions: &Path, days: i64) -> PathBuf {
    let (y, m, d) = civil_from_days(days);
    sessions
        .join(format!("{y:04}"))
        .join(format!("{m:02}"))
        .join(format!("{d:02}"))
}

pub fn day_folders(sessions: &Path, now_ms: i64, days: u32) -> Vec<PathBuf> {
    let today = now_ms.div_euclid(86_400_000);
    (0..i64::from(days))
        .map(|back| day_folder(sessions, today - back))
        .filter(|p| p.is_dir())
        .collect()
}

pub trait ChildSource: Send + Sync {
    fn children(&self, parent_thread: &str, parent_path: &Path) -> Vec<Head>;

    fn done(&self) {}
}

impl ChildSource for Index {
    fn children(&self, parent_thread: &str, _parent_path: &Path) -> Vec<Head> {
        Index::children(self, parent_thread)
    }
}

pub struct DayFolders;

const KNOWN_HEADS: usize = 20_000;

fn known_heads() -> &'static Mutex<HashMap<PathBuf, Head>> {
    static HEADS: OnceLock<Mutex<HashMap<PathBuf, Head>>> = OnceLock::new();
    HEADS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn known_head(path: &Path) -> Option<Head> {
    if let Some(hit) = lock(known_heads()).get(path) {
        return Some(hit.clone());
    }
    let read = head(path)?;
    let mut heads = lock(known_heads());
    if heads.len() >= KNOWN_HEADS {
        heads.clear();
    }
    heads.insert(path.to_path_buf(), read.clone());
    Some(read)
}

fn day_of(path: &Path) -> Option<(PathBuf, i64)> {
    let day = path.parent()?;
    let number = |p: Option<&Path>| -> Option<u32> { p?.file_name()?.to_str()?.parse().ok() };
    let d = number(Some(day))?;
    let m = number(day.parent())?;
    let y = number(day.parent()?.parent())?;
    Some((
        day.parent()?.parent()?.parent()?.to_path_buf(),
        days_from_civil(i64::from(y), m, d),
    ))
}

impl ChildSource for DayFolders {
    fn children(&self, parent_thread: &str, parent_path: &Path) -> Vec<Head> {
        if parent_thread.is_empty() {
            return Vec::new();
        }
        let Some((sessions, day)) = day_of(parent_path) else {
            return Vec::new();
        };
        let mut found: Vec<Head> = [day, day + 1]
            .into_iter()
            .map(|d| day_folder(&sessions, d))
            .flat_map(|folder| rollout_files(&folder))
            .filter_map(|file| known_head(&file))
            .filter(|h| h.parent.as_deref() == Some(parent_thread))
            .collect();
        found.sort_by(|a, b| a.started.total_cmp(&b.started).then(a.path.cmp(&b.path)));
        found
    }
}
