use super::model::{
    new_id, Change, InsertError, Kind, New, Row, State, Tool, Via, Why, MAX_ROWS, MAX_TEXT,
};
use crate::store;
use crate::transcript::sent::Seen;
use rusqlite::{params, Connection, TransactionBehavior};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const COLUMNS: &str = "n, id, session, tool, kind, text, options, via, state, why, clear_box, \
    queued_ms, updated_ms, attempt_ms, seen_before, queued_before, tries, next_ms, noticed_ms";

const EDITABLE: &[State] = &[
    State::Queued,
    State::Waiting,
    State::Box,
    State::Stopped,
    State::Gone,
];

pub struct OutboxStore {
    db: Mutex<Connection>,
}

fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

fn column_error(index: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        format!("unknown {what}").into(),
    )
}

fn word<T>(
    r: &rusqlite::Row<'_>,
    index: usize,
    what: &str,
    parse: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let text: String = r.get(index)?;
    parse(&text).ok_or_else(|| column_error(index, what))
}

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let options: Option<String> = r.get(6)?;
    let why: Option<String> = r.get(9)?;
    let tries: i64 = r.get(16)?;
    Ok(Row {
        n: r.get(0)?,
        id: r.get(1)?,
        session: r.get(2)?,
        tool: word(r, 3, "tool", Tool::parse)?,
        kind: word(r, 4, "kind", Kind::parse)?,
        text: r.get(5)?,
        options: match options {
            Some(text) => {
                Some(serde_json::from_str(&text).map_err(|_| column_error(6, "options"))?)
            }
            None => None,
        },
        via: word(r, 7, "via", Via::parse)?,
        state: word(r, 8, "state", State::parse)?,
        why: match why {
            Some(text) => Some(Why::parse(&text).ok_or_else(|| column_error(9, "why"))?),
            None => None,
        },
        clear_box: r.get::<_, i64>(10)? != 0,
        queued_ms: r.get(11)?,
        updated_ms: r.get(12)?,
        attempt_ms: r.get(13)?,
        seen_before: r.get(14)?,
        queued_before: r.get(15)?,
        tries: u32::try_from(tries).unwrap_or(0),
        next_ms: r.get(17)?,
        noticed_ms: r.get(18)?,
    })
}

fn states(list: &[State]) -> String {
    list.iter()
        .map(|state| format!("'{}'", state.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fetch(db: &Connection, id: &str) -> Result<Option<Row>, String> {
    let sql = format!("SELECT {COLUMNS} FROM outbox WHERE id = ?1");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let mut rows = query.query_map([id], row_of).map_err(db_err)?;
    rows.next().transpose().map_err(db_err)
}

fn select(db: &Connection, tail: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Row>, String> {
    let sql = format!("SELECT {COLUMNS} FROM outbox {tail}");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let rows = query.query_map(args, row_of).map_err(db_err)?;
    rows.collect::<Result<_, _>>().map_err(db_err)
}

impl OutboxStore {
    pub fn open(data_dir: &Path) -> Result<OutboxStore, String> {
        Ok(OutboxStore {
            db: Mutex::new(store::open(data_dir)?),
        })
    }

    pub fn in_memory_for_tests() -> OutboxStore {
        let db = Connection::open_in_memory().expect("in-memory database");
        store::migrate(&db).expect("migrate");
        OutboxStore { db: Mutex::new(db) }
    }

    pub fn run_sql_for_tests(&self, sql: &str) {
        self.lock().execute_batch(sql).expect("test sql");
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, new: &New, now_ms: i64) -> Result<Row, InsertError> {
        if new.text.trim().is_empty() {
            return Err(InsertError::Empty);
        }
        if new.text.len() > MAX_TEXT {
            return Err(InsertError::TooLong);
        }
        let options = new.options.as_ref().map(|value| value.to_string());
        let id = new_id();
        let kind = Kind::of(new.tool, &new.text);
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| InsertError::Db(e.to_string()))?;
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE session = ?1",
                [&new.session],
                |r| r.get(0),
            )
            .map_err(|e| InsertError::Db(e.to_string()))?;
        if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_ROWS {
            return Err(InsertError::Full);
        }
        tx.execute(
            "INSERT INTO outbox (id, session, tool, kind, text, options, via, state, clear_box, queued_ms, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?9, ?9)",
            params![
                id,
                new.session,
                new.tool.as_str(),
                kind.as_str(),
                new.text,
                options,
                new.via.as_str(),
                i64::from(new.clear_box),
                now_ms
            ],
        )
        .map_err(|e| InsertError::Db(e.to_string()))?;
        let row = fetch(&tx, &id).map_err(InsertError::Db)?;
        tx.commit().map_err(|e| InsertError::Db(e.to_string()))?;
        row.ok_or_else(|| InsertError::Db("row missing after insert".to_string()))
    }

    pub fn list(&self, session: Option<&str>) -> Result<Vec<Row>, String> {
        let db = self.lock();
        match session {
            Some(session) => select(&db, "WHERE session = ?1 ORDER BY n", &[&session]),
            None => select(&db, "ORDER BY n", &[]),
        }
    }

    pub fn get(&self, id: &str) -> Result<Option<Row>, String> {
        fetch(&self.lock(), id)
    }

    pub fn head(&self, session: &str) -> Result<Option<Row>, String> {
        let db = self.lock();
        let rows = select(&db, "WHERE session = ?1 ORDER BY n LIMIT 1", &[&session])?;
        Ok(rows.into_iter().next())
    }

    pub fn sessions(&self) -> Result<Vec<String>, String> {
        let db = self.lock();
        let mut query = db
            .prepare_cached("SELECT session FROM outbox GROUP BY session ORDER BY MIN(n)")
            .map_err(db_err)?;
        let rows = query.query_map([], |r| r.get(0)).map_err(db_err)?;
        rows.collect::<Result<_, _>>().map_err(db_err)
    }

    pub fn take(&self, id: &str, now_ms: i64) -> Result<Option<Row>, String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let changed = tx
            .execute(
                "UPDATE outbox SET state = 'sending', attempt_ms = ?2, seen_before = NULL,
                     queued_before = NULL, tries = tries + 1, why = NULL, updated_ms = ?2
                 WHERE id = ?1 AND state IN ('queued', 'waiting')
                   AND (next_ms IS NULL OR next_ms <= ?2)",
                params![id, now_ms],
            )
            .map_err(db_err)?;
        if changed == 0 {
            return Ok(None);
        }
        let row = fetch(&tx, id)?;
        tx.commit().map_err(db_err)?;
        Ok(row)
    }

    pub fn set_seen(&self, id: &str, seen: Seen) -> Result<(), String> {
        let to_i64 = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        self.lock()
            .execute(
                "UPDATE outbox SET seen_before = ?2, queued_before = ?3
                 WHERE id = ?1 AND state = 'sending'",
                params![id, to_i64(seen.taken), to_i64(seen.enqueued)],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn set_state(
        &self,
        id: &str,
        from: &[State],
        to: State,
        why: Option<Why>,
        next_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<bool, String> {
        let sql = format!(
            "UPDATE outbox SET state = ?2, why = ?3, next_ms = ?4, updated_ms = ?5
             WHERE id = ?1 AND state IN ({})",
            states(from)
        );
        let changed = self
            .lock()
            .execute(
                &sql,
                params![id, to.as_str(), why.map(Why::as_str), next_ms, now_ms],
            )
            .map_err(db_err)?;
        Ok(changed > 0)
    }

    pub fn edit(&self, id: &str, text: &str, now_ms: i64) -> Result<Change, String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let Some(row) = fetch(&tx, id)? else {
            return Ok(Change::Missing);
        };
        let sql = format!(
            "UPDATE outbox SET text = ?2, kind = ?3, tries = 0, next_ms = NULL, clear_box = 0,
                 why = NULL, state = 'queued', updated_ms = ?4
             WHERE id = ?1 AND state IN ({})",
            states(EDITABLE)
        );
        let kind = Kind::of(row.tool, text);
        let changed = tx
            .execute(&sql, params![id, text, kind.as_str(), now_ms])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(if changed > 0 {
            Change::Done
        } else {
            Change::Busy
        })
    }

    pub fn remove(&self, id: &str) -> Result<(Change, Option<Row>), String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let Some(row) = fetch(&tx, id)? else {
            return Ok((Change::Missing, None));
        };
        let changed = tx
            .execute(
                "DELETE FROM outbox WHERE id = ?1 AND state NOT IN ('sending', 'in_queue')",
                [id],
            )
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(if changed > 0 {
            (Change::Done, Some(row))
        } else {
            (Change::Busy, None)
        })
    }

    pub fn requeue(
        &self,
        id: &str,
        from: &[State],
        clear_box: bool,
        now_ms: i64,
    ) -> Result<Change, String> {
        self.requeue_with(id, from, clear_box, None, now_ms)
    }

    pub fn untake(
        &self,
        id: &str,
        clear_box: bool,
        tries: u32,
        now_ms: i64,
    ) -> Result<Change, String> {
        self.requeue_with(id, &[State::Sending], clear_box, Some(tries), now_ms)
    }

    fn requeue_with(
        &self,
        id: &str,
        from: &[State],
        clear_box: bool,
        tries: Option<u32>,
        now_ms: i64,
    ) -> Result<Change, String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        if fetch(&tx, id)?.is_none() {
            return Ok(Change::Missing);
        }
        let sql = format!(
            "UPDATE outbox SET state = 'queued', clear_box = ?2, why = NULL, attempt_ms = NULL,
                 seen_before = NULL, queued_before = NULL, next_ms = NULL, updated_ms = ?3,
                 tries = COALESCE(?4, tries)
             WHERE id = ?1 AND state IN ({})",
            states(from)
        );
        let changed = tx
            .execute(
                &sql,
                params![id, i64::from(clear_box), now_ms, tries.map(i64::from)],
            )
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(if changed > 0 {
            Change::Done
        } else {
            Change::Busy
        })
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let changed = self
            .lock()
            .execute("DELETE FROM outbox WHERE id = ?1", [id])
            .map_err(db_err)?;
        Ok(changed > 0)
    }

    pub fn discard_session(&self, session: &str) -> Result<Vec<Row>, String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let rows = select(
            &tx,
            "WHERE session = ?1 AND state <> 'sending' ORDER BY n",
            &[&session],
        )?;
        tx.execute(
            "DELETE FROM outbox WHERE session = ?1 AND state <> 'sending'",
            [session],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(rows)
    }

    pub fn discard_rows(&self, ids: &[String]) -> Result<Vec<Row>, String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let mut gone = Vec::new();
        for id in ids {
            let Some(row) = fetch(&tx, id)? else {
                continue;
            };
            if row.state == State::Sending {
                continue;
            }
            tx.execute("DELETE FROM outbox WHERE id = ?1", [id])
                .map_err(db_err)?;
            gone.push(row);
        }
        tx.commit().map_err(db_err)?;
        Ok(gone)
    }

    pub fn rekey(&self, from: &str, to: &str) -> Result<usize, String> {
        self.lock()
            .execute(
                "UPDATE outbox SET session = ?2 WHERE session = ?1",
                params![from, to],
            )
            .map_err(db_err)
    }

    pub fn unnoticed_before(&self, queued_before_ms: i64) -> Result<Vec<Row>, String> {
        let db = self.lock();
        select(
            &db,
            "WHERE queued_ms < ?1 AND noticed_ms IS NULL ORDER BY n",
            &[&queued_before_ms],
        )
    }

    pub fn mark_noticed(&self, ids: &[String], now_ms: i64) -> Result<(), String> {
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        for id in ids {
            tx.execute(
                "UPDATE outbox SET noticed_ms = ?2 WHERE id = ?1",
                params![id, now_ms],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    pub fn sending(&self) -> Result<Vec<Row>, String> {
        let db = self.lock();
        select(&db, "WHERE state = 'sending' ORDER BY n", &[])
    }
}
