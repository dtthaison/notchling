use super::{Book, Change, Item, State};
use crate::store;
use rusqlite::{params, TransactionBehavior};
use std::path::Path;

pub fn load(data_dir: &Path, now_ms: i64) -> Result<Book, String> {
    let Some(db) = store::open_existing(data_dir)? else {
        return Ok(Book::new());
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
        if item.state == State::Open {
            item.state = State::Withdrawn;
            item.closed_ms = Some(now_ms);
        }
        items.push(item);
    }
    Ok(Book::from_items(items))
}

pub fn save(data_dir: &Path, change: &Change) -> Result<(), String> {
    if matches!(change, Change::Removed(_)) && !store::exists(data_dir) {
        return Ok(());
    }
    let mut db = store::open(data_dir)?;
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
