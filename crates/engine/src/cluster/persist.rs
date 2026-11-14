use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

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
    Ok(Some(ClusterState {
        id: read(&id)?,
        name: read(&name)?,
        term: read::<Term>(&term)?,
        home: read::<NodeId>(&home)?,
        vault: read::<NodeId>(&vault)?,
        members,
        jobs,
        seq: u64::try_from(seq).map_err(|e| e.to_string())?,
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

#[cfg(test)]
#[path = "persist_tests.rs"]
mod persist_tests;
