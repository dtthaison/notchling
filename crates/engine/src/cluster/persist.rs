use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use super::claims::{Claim, SideCopy};
use super::failover::{ArmingId, Failover};
use super::held::{Delivered, Held, HeldQueue};
use super::id::NodeId;
use super::jobs::Job;
use super::members::{Member, Term};
use super::state::{Change, ClusterState, JournalEntry};
use super::versions::CLUSTER_SCHEMA;
use crate::store;

pub const CLUSTER_DIR: &str = "cluster";
pub const CLUSTER_DB: &str = "cluster.db";

fn db_path(dir: &Path) -> PathBuf {
    dir.join(CLUSTER_DB)
}

fn newer() -> String {
    "cluster.db is from a newer Notchling".to_string()
}

fn migrate(db: &Connection) -> Result<(), String> {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if version > i64::from(CLUSTER_SCHEMA) {
        return Err(newer());
    }
    if version == i64::from(CLUSTER_SCHEMA) {
        return Ok(());
    }
    db.execute_batch(&format!(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS members (id TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS jobs (
             id TEXT PRIMARY KEY, key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS journal (
             seq INTEGER PRIMARY KEY, at_ms INTEGER NOT NULL,
             term INTEGER NOT NULL, command TEXT NOT NULL);
         PRAGMA user_version = {CLUSTER_SCHEMA};
         COMMIT;"
    ))
    .map_err(|e| e.to_string())
}

fn stored_version(path: &Path) -> Result<i64, String> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    db.pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())
}

fn open(dir: &Path, create: bool) -> Result<Option<Connection>, String> {
    let path = db_path(dir);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "refusing to open a database through a symlink: {}",
                    path.display()
                ));
            }
            if stored_version(&path)? > i64::from(CLUSTER_SCHEMA) {
                return Err(newer());
            }
        }
        Err(_) if !create => return Ok(None),
        Err(_) => {}
    }
    store::open_db(&path, &migrate).map(Some)
}

const INBOX_TABLE: &str = "CREATE TABLE IF NOT EXISTS inbox (
    id TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL)";

pub fn open_inbox(dir: &Path, create: bool) -> Result<Option<Connection>, String> {
    let Some(db) = open(dir, create)? else {
        return Ok(None);
    };
    db.execute_batch(INBOX_TABLE).map_err(|e| e.to_string())?;
    Ok(Some(db))
}

const HELD_STATE: &str = "held";
const DELIVERED_STATE: &str = "delivered";

const HELD_TABLE: &str = "CREATE TABLE IF NOT EXISTS held (
    id TEXT PRIMARY KEY, seq INTEGER NOT NULL, state TEXT NOT NULL,
    confirmed INTEGER NOT NULL, value TEXT NOT NULL)";

pub fn open_held(dir: &Path, create: bool) -> Result<Option<Connection>, String> {
    let Some(db) = open(dir, create)? else {
        return Ok(None);
    };
    db.execute_batch(HELD_TABLE).map_err(|e| e.to_string())?;
    Ok(Some(db))
}

pub fn load_held(cluster_dir: &Path) -> Result<HeldQueue, String> {
    let Some(db) = open_held(cluster_dir, false)? else {
        return Ok(HeldQueue::default());
    };
    let mut query = db
        .prepare("SELECT state, confirmed, value FROM held ORDER BY seq")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut ranked: Vec<(i64, String)> = Vec::new();
    let mut delivered = Vec::new();
    for row in rows {
        let (state, rank, value) = row.map_err(|e| e.to_string())?;
        match state.as_str() {
            HELD_STATE => {
                let held: Held = read(&value)?;
                if rank > 0 {
                    ranked.push((rank, held.id.clone()));
                }
                entries.push(held);
            }
            DELIVERED_STATE => delivered.push(read::<Delivered>(&value)?),
            other => return Err(format!("held row in an unknown state: {other}")),
        }
    }
    ranked.sort();
    Ok(HeldQueue {
        entries,
        confirmed: ranked.into_iter().map(|(_, id)| id).collect(),
        delivered,
    })
}

pub fn save_held(cluster_dir: &Path, queue: &HeldQueue) -> Result<(), String> {
    let Some(mut db) = open_held(cluster_dir, true)? else {
        return Err("cluster.db could not be opened".to_string());
    };
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM held", [])
        .map_err(|e| e.to_string())?;
    let insert = "INSERT INTO held (id, seq, state, confirmed, value) VALUES (?1, ?2, ?3, ?4, ?5)";
    for (seq, done) in queue.delivered.iter().enumerate() {
        tx.execute(
            insert,
            params![done.id, seq as i64, DELIVERED_STATE, 0, text(done)?],
        )
        .map_err(|e| e.to_string())?;
    }
    for (seq, held) in queue.entries.iter().enumerate() {
        let rank = queue
            .confirmed
            .iter()
            .position(|c| *c == held.id)
            .map_or(0, |at| at as i64 + 1);
        tx.execute(
            insert,
            params![held.id, seq as i64, HELD_STATE, rank, text(held)?],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn text<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn read<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

pub fn load(cluster_dir: &Path) -> Result<Option<ClusterState>, String> {
    let Some(db) = open(cluster_dir, false)? else {
        return Ok(None);
    };
    let get = |key: &str| -> Result<Option<String>, String> {
        db.query_row("SELECT value FROM state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())
    };
    let (Some(id), Some(name), Some(term), Some(home), Some(vault)) = (
        get("id")?,
        get("name")?,
        get("term")?,
        get("home")?,
        get("vault")?,
    ) else {
        return Ok(None);
    };
    let mut members = BTreeMap::new();
    let mut query = db
        .prepare("SELECT value FROM members")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let member: Member = read(&row.map_err(|e| e.to_string())?)?;
        members.insert(member.id.clone(), member);
    }
    let mut jobs = BTreeMap::new();
    let mut query = db
        .prepare("SELECT value FROM jobs")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let job: Job = read(&row.map_err(|e| e.to_string())?)?;
        jobs.insert(job.id.clone(), job);
    }
    let seq: i64 = db
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM journal", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    fn stored<T: serde::de::DeserializeOwned + Default>(
        value: Option<String>,
    ) -> Result<T, String> {
        value.map_or_else(|| Ok(T::default()), |v| read(&v))
    }

    let failover = match get("failover")? {
        Some(value) => read::<Failover>(&value)?,
        None => Failover::Off,
    };
    let armed = match get("armed")? {
        Some(value) => read::<Option<(NodeId, Term)>>(&value)?,
        None => None,
    };
    let holders = match get("holders")? {
        Some(value) => read::<BTreeMap<NodeId, ArmingId>>(&value)?,
        None => seeded_holders(&armed),
    };
    let claims = match get("claims")? {
        Some(value) => read::<BTreeMap<String, Claim>>(&value)?,
        None => BTreeMap::new(),
    };
    let side_copies = match get("side_copies")? {
        Some(value) => read::<Vec<SideCopy>>(&value)?,
        None => Vec::new(),
    };
    let term = read::<Term>(&term)?;
    let terms = std::iter::once(&term)
        .chain(armed.iter().map(|(_, t)| t))
        .chain(holders.values().map(|h| &h.term))
        .chain(jobs.values().map(|j| &j.term))
        .chain(claims.values().map(|c| &c.term));
    for stored in terms {
        if !stored.storable() {
            return Err(format!(
                "cluster.db holds a term the journal can't store: {}",
                stored.n
            ));
        }
    }
    Ok(Some(ClusterState {
        id: read(&id)?,
        name: read(&name)?,
        term,
        home: read::<NodeId>(&home)?,
        vault: read::<NodeId>(&vault)?,
        members,
        jobs,
        seq: u64::try_from(seq).map_err(|e| e.to_string())?,
        main: stored(get("main")?)?,
        kept: stored(get("kept")?)?,
        ended: stored(get("ended")?)?,
        failover,
        armed,
        holders,
        claims,
        side_copies,
    }))
}

pub fn save(cluster_dir: &Path, entry: &JournalEntry, changes: &[Change]) -> Result<(), String> {
    let Some(mut db) = open(cluster_dir, true)? else {
        return Err("cluster.db could not be opened".to_string());
    };
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let last: i64 = tx
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM journal", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    let seq = i64::try_from(entry.seq).map_err(|e| e.to_string())?;
    if seq <= last {
        return Err(format!("journal entry {seq} is not after {last}"));
    }
    let term = i64::try_from(entry.term.n).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO journal (seq, at_ms, term, command) VALUES (?1, ?2, ?3, ?4)",
        params![seq, entry.at_ms, term, text(&entry.command)?],
    )
    .map_err(|e| e.to_string())?;
    for change in changes {
        apply(&tx, change)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn apply(tx: &rusqlite::Transaction<'_>, change: &Change) -> Result<(), String> {
    let es = |e: rusqlite::Error| e.to_string();
    match change {
        Change::Member(m) => {
            tx.execute(
                "INSERT INTO members (id, value) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value",
                params![m.id.as_str(), text(m)?],
            )
            .map_err(es)?;
        }
        Change::Dropped(id) => {
            tx.execute("DELETE FROM members WHERE id = ?1", [id.as_str()])
                .map_err(es)?;
        }
        Change::Job(j) => {
            tx.execute(
                "INSERT INTO jobs (id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value",
                params![j.id, j.key, text(j)?],
            )
            .map_err(es)?;
        }
        Change::Main { main, kept, ended } => {
            for (key, value) in [
                ("main", text(main)?),
                ("kept", text(kept)?),
                ("ended", text(ended)?),
            ] {
                tx.execute(
                    "INSERT INTO state (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )
                .map_err(es)?;
            }
        }
        Change::Failover(failover) => put_state(tx, "failover", &text(failover)?)?,
        Change::Armed(armed) => {
            keep_seeded_holders(tx)?;
            put_state(tx, "armed", &text(armed)?)?;
        }
        Change::Holders(holders) => put_state(tx, "holders", &text(holders)?)?,
        Change::Claims(claims) => put_state(tx, "claims", &text(claims)?)?,
        Change::SideCopies(copies) => put_state(tx, "side_copies", &text(copies)?)?,
        Change::Cluster {
            id,
            name,
            term,
            home,
            vault,
        } => {
            for (key, value) in [
                ("id", text(id)?),
                ("name", text(name)?),
                ("term", text(term)?),
                ("home", text(home)?),
                ("vault", text(vault)?),
            ] {
                tx.execute(
                    "INSERT INTO state (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )
                .map_err(es)?;
            }
        }
    }
    Ok(())
}

fn seeded_holders(armed: &Option<(NodeId, Term)>) -> BTreeMap<NodeId, ArmingId> {
    armed
        .iter()
        .map(|(node, term)| {
            (
                node.clone(),
                ArmingId {
                    term: term.clone(),
                    seq: 0,
                },
            )
        })
        .collect()
}

fn keep_seeded_holders(tx: &rusqlite::Transaction<'_>) -> Result<(), String> {
    let es = |e: rusqlite::Error| e.to_string();
    let value = |key: &str| -> Result<Option<String>, String> {
        tx.query_row("SELECT value FROM state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(es)
    };
    if value("holders")?.is_some() {
        return Ok(());
    }
    let Some(armed) = value("armed")? else {
        return Ok(());
    };
    let armed = read::<Option<(NodeId, Term)>>(&armed)?;
    if armed.is_some() {
        put_state(tx, "holders", &text(&seeded_holders(&armed))?)?;
    }
    Ok(())
}

fn put_state(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<(), String> {
    tx.execute(
        "INSERT INTO state (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

pub fn wipe(cluster_dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(cluster_dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err("refusing to wipe through a symlink".to_string());
        }
        Ok(_) => {}
        Err(_) => return Ok(()),
    }
    for name in [
        CLUSTER_DB.to_string(),
        format!("{CLUSTER_DB}-wal"),
        format!("{CLUSTER_DB}-shm"),
    ] {
        match std::fs::remove_file(cluster_dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

pub fn keep_aside(cluster_dir: &Path, now_ms: i64) -> Result<(), String> {
    match std::fs::symlink_metadata(cluster_dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err("refusing to move files through a symlink".to_string());
        }
        Ok(_) => {}
        Err(_) => return Ok(()),
    }
    for suffix in ["", "-wal", "-shm"] {
        let from = cluster_dir.join(format!("{CLUSTER_DB}{suffix}"));
        let to = cluster_dir.join(format!("{CLUSTER_DB}.inbox-left-{now_ms}{suffix}"));
        match std::fs::rename(&from, &to) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod persist_tests;
