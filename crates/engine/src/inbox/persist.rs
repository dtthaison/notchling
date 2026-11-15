use super::book::cut;
use super::{Book, Change, Item, State, DETAIL_MAX, TITLE_MAX};
use crate::cluster::persist::{self as cluster_persist, CLUSTER_DB};
use crate::store;
use rusqlite::{params, Connection, TransactionBehavior};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Store {
    Node(PathBuf),
    Cluster(PathBuf),
}

impl Store {
    fn open(&self, create: bool) -> Result<Option<Connection>, String> {
        match self {
            Store::Node(dir) if create => store::open(dir).map(Some),
            Store::Node(dir) => store::open_existing(dir),
            Store::Cluster(dir) => cluster_persist::open_inbox(dir, create),
        }
    }
}

pub fn load(data_dir: &Path, now_ms: i64) -> Result<Book, String> {
    load_from(&Store::Node(data_dir.to_path_buf()), now_ms)
}

pub fn load_from(from: &Store, now_ms: i64) -> Result<Book, String> {
    let mut items = read_from(from)?;
    for item in &mut items {
        if item.state == State::Open {
            item.state = State::Withdrawn;
            item.closed_ms = Some(now_ms);
        }
    }
    Ok(Book::from_items(items))
}

pub fn read_from(from: &Store) -> Result<Vec<Item>, String> {
    let Some(db) = from.open(false)? else {
        return Ok(Vec::new());
    };
    let mut query = db
        .prepare("SELECT value FROM inbox ORDER BY rowid")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let text = row.map_err(|e| e.to_string())?;
        let Ok(mut item) = serde_json::from_str::<Item>(&text) else {
            continue;
        };
        item.title = cut(item.title, TITLE_MAX);
        item.detail = cut(item.detail, DETAIL_MAX);
        items.push(item);
    }
    Ok(items)
}

pub fn save(data_dir: &Path, change: &Change) -> Result<(), String> {
    save_to(&Store::Node(data_dir.to_path_buf()), change)
}

pub fn save_to(to: &Store, change: &Change) -> Result<(), String> {
    let Some(mut db) = to.open(!matches!(change, Change::Removed(_)))? else {
        return Ok(());
    };
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    match change {
        Change::Raised(item) | Change::Updated(item) => {
            let value = serde_json::to_string(item).map_err(|e| e.to_string())?;
            let updated_ms = item.closed_ms.unwrap_or(item.created_ms);
            tx.execute(
                "INSERT INTO inbox (id, value, updated_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms",
                params![item.id, value, updated_ms],
            )
        }
        Change::Removed(id) => tx.execute("DELETE FROM inbox WHERE id = ?1", [id]),
    }
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn migrate_inbox(notchling_db: &Path, cluster_db: &Path) -> Result<usize, String> {
    let (data_dir, cluster_dir) = folders(notchling_db, cluster_db)?;
    move_rows(&Store::Node(data_dir), &Store::Cluster(cluster_dir))
}

pub fn migrate_inbox_back(cluster_db: &Path, notchling_db: &Path) -> Result<usize, String> {
    let (data_dir, cluster_dir) = folders(notchling_db, cluster_db)?;
    move_rows(&Store::Cluster(cluster_dir), &Store::Node(data_dir))
}

fn folders(notchling_db: &Path, cluster_db: &Path) -> Result<(PathBuf, PathBuf), String> {
    if cluster_db.file_name().and_then(|n| n.to_str()) != Some(CLUSTER_DB) {
        return Err(format!("{} is not {CLUSTER_DB}", cluster_db.display()));
    }
    if notchling_db.file_name().and_then(|n| n.to_str()) != Some(store::DB_NAME) {
        return Err(format!(
            "{} is not {}",
            notchling_db.display(),
            store::DB_NAME
        ));
    }
    match (notchling_db.parent(), cluster_db.parent()) {
        (Some(data), Some(cluster)) => Ok((data.to_path_buf(), cluster.to_path_buf())),
        _ => Err("the databases need a folder".to_string()),
    }
}

type Row = (String, String, i64);

fn read_rows(db: &Connection) -> Result<Vec<Row>, String> {
    let mut query = db
        .prepare("SELECT id, value, updated_ms FROM inbox ORDER BY rowid")
        .map_err(|e| e.to_string())?;
    let found = query
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<Row>, _>>()
        .map_err(|e| e.to_string());
    found
}

fn move_rows(from: &Store, to: &Store) -> Result<usize, String> {
    let Some(mut source) = from.open(false)? else {
        return Ok(0);
    };
    let rows = read_rows(&source)?;
    if rows.is_empty() {
        return Ok(0);
    }
    let Some(mut target) = to.open(true)? else {
        return Err("the inbox's new home could not be opened".to_string());
    };
    let tx = target
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut copied = 0;
    for (id, value, updated_ms) in &rows {
        copied += tx
            .execute(
                "INSERT INTO inbox (id, value, updated_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms
                 WHERE excluded.updated_ms > inbox.updated_ms",
                params![id, value, updated_ms],
            )
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    let tx = source
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    for (id, _, _) in &rows {
        tx.execute("DELETE FROM inbox WHERE id = ?1", [id])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(copied)
}
