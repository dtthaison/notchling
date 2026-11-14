use notchling_engine::activity::{DbQuery, EventsDb};
use notchling_engine::cost::account_log::{AccountSwitches, Entry};
use std::sync::{Arc, Mutex, OnceLock};

const KIND: &str = "account.switched";
const MOST: usize = 10_000;

#[derive(Default)]
pub struct DbSwitches {
    db: OnceLock<Arc<Mutex<EventsDb>>>,
}

impl DbSwitches {
    pub fn new() -> Arc<DbSwitches> {
        Arc::new(DbSwitches::default())
    }

    pub fn set(&self, db: Arc<Mutex<EventsDb>>) {
        let _ = self.db.set(db);
    }
}

impl AccountSwitches for DbSwitches {
    fn switches(&self) -> Vec<Entry> {
        let Some(db) = self.db.get() else {
            return Vec::new();
        };
        let rows = {
            let db = db.lock().unwrap_or_else(|e| e.into_inner());
            db.list(&DbQuery {
                kinds: vec![KIND.to_owned()],
                limit: MOST,
                ..DbQuery::default()
            })
        };
        let mut entries: Vec<Entry> = rows
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.outcome == "ok")
            .filter_map(|row| {
                let f = row.fields();
                let id = f
                    .get("to")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.is_empty())?;
                let claude = f
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|p| p == "claude");
                claude.then(|| Entry {
                    at: row.ts as f64 / 1000.0,
                    id: id.to_owned(),
                })
            })
            .collect();
        entries.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }
}

#[cfg(test)]
#[path = "account_history_tests.rs"]
mod tests;
