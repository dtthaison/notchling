use super::model::{
    new_id, AskState, Asker, By, Found, Mode, New, OpenError, Row, KEEP_CLOSED_MS, MAX_OPEN,
    MAX_OPEN_PER_SESSION,
};
use crate::store;
use rusqlite::{params, Connection, TransactionBehavior};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const COLUMNS: &str = "n, id, key, session, session_id, asker, agent_id, label, agent_type, \
    model, tool_use_id, folder, branch, question, choices, main_may_answer, mode, deadline_ms, \
    pid, pid_start, state, answer, choice, by, inbox_id, created_ms, updated_ms, closed_ms";

const CLOSED: &[AskState] = &[
    AskState::Delivered,
    AskState::Dismissed,
    AskState::Expired,
    AskState::Gone,
];

pub struct AskStore {
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
    let choices: Option<String> = r.get(14)?;
    let pid: i64 = r.get(18)?;
    let pid_start: Option<i64> = r.get(19)?;
    let choice: Option<i64> = r.get(22)?;
    let by: Option<String> = r.get(23)?;
    Ok(Row {
        n: r.get(0)?,
        id: r.get(1)?,
        key: r.get(2)?,
        session: r.get(3)?,
        session_id: r.get(4)?,
        asker: word(r, 5, "asker", Asker::parse)?,
        agent_id: r.get(6)?,
        label: r.get(7)?,
        agent_type: r.get(8)?,
        model: r.get(9)?,
        tool_use_id: r.get(10)?,
        folder: r.get(11)?,
        branch: r.get(12)?,
        question: r.get(13)?,
        choices: match choices {
            Some(text) => {
                Some(serde_json::from_str(&text).map_err(|_| column_error(14, "choices"))?)
            }
            None => None,
        },
        main_may_answer: r.get::<_, i64>(15)? != 0,
        mode: word(r, 16, "mode", Mode::parse)?,
        deadline_ms: r.get(17)?,
        pid: u32::try_from(pid).unwrap_or(0),
        pid_start: pid_start.and_then(|n| u64::try_from(n).ok()),
        state: word(r, 20, "state", AskState::parse)?,
        answer: r.get(21)?,
        choice: choice.and_then(|n| u8::try_from(n).ok()),
        by: match by {
            Some(text) => Some(By::parse(&text).ok_or_else(|| column_error(23, "by"))?),
            None => None,
        },
        inbox_id: r.get(24)?,
        created_ms: r.get(25)?,
        updated_ms: r.get(26)?,
        closed_ms: r.get(27)?,
    })
}

fn states(list: &[AskState]) -> String {
    list.iter()
        .map(|state| format!("'{}'", state.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fetch(db: &Connection, id: &str) -> Result<Option<Row>, String> {
    let sql = format!("SELECT {COLUMNS} FROM asks WHERE id = ?1");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let mut rows = query.query_map([id], row_of).map_err(db_err)?;
    rows.next().transpose().map_err(db_err)
}

fn select(db: &Connection, tail: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Row>, String> {
    let sql = format!("SELECT {COLUMNS} FROM asks {tail}");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let rows = query.query_map(args, row_of).map_err(db_err)?;
    rows.collect::<Result<_, _>>().map_err(db_err)
}

fn count(tx: &Connection, session: Option<&str>) -> Result<usize, OpenError> {
    let live = "state IN ('open', 'answered')";
    let found: i64 = match session {
        Some(session) => tx.query_row(
            &format!("SELECT COUNT(*) FROM asks WHERE {live} AND session = ?1"),
            [session],
            |r| r.get(0),
        ),
        None => tx.query_row(
            &format!("SELECT COUNT(*) FROM asks WHERE {live}"),
            [],
            |r| r.get(0),
        ),
    }
    .map_err(|e| OpenError::Db(e.to_string()))?;
    Ok(usize::try_from(found).unwrap_or(usize::MAX))
}

impl AskStore {
    pub fn open(data_dir: &Path) -> Result<AskStore, String> {
        Ok(AskStore {
            db: Mutex::new(store::open(data_dir)?),
        })
    }

    pub fn in_memory_for_tests() -> AskStore {
        let db = Connection::open_in_memory().expect("in-memory database");
        store::migrate(&db).expect("migrate");
        AskStore { db: Mutex::new(db) }
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, new: &New, now_ms: i64) -> Result<Row, OpenError> {
        let choices = new
            .choices
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok());
        let db_error = |e: rusqlite::Error| OpenError::Db(e.to_string());
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let same = select(
            &tx,
            "WHERE key = ?1 AND pid = ?2 ORDER BY n LIMIT 1",
            &[&new.key, &i64::from(new.pid)],
        )
        .map_err(OpenError::Db)?;
        if let Some(row) = same.into_iter().next() {
            return Ok(row);
        }
        if count(&tx, Some(&new.session))? >= MAX_OPEN_PER_SESSION {
            return Err(OpenError::SessionFull);
        }
        if count(&tx, None)? >= MAX_OPEN {
            return Err(OpenError::AllFull);
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO asks (id, key, session, session_id, asker, folder, branch, question,
                 choices, main_may_answer, mode, deadline_ms, pid, pid_start, state,
                 created_ms, updated_ms)
             VALUES (?1, ?2, ?3, ?4, 'unknown', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'open',
                 ?14, ?14)",
            params![
                id,
                new.key,
                new.session,
                new.session_id,
                new.folder,
                new.branch,
                new.question,
                choices,
                i64::from(new.main_may_answer),
                new.mode.as_str(),
                new.deadline_ms,
                i64::from(new.pid),
                new.pid_start.and_then(|n| i64::try_from(n).ok()),
                now_ms
            ],
        )
        .map_err(db_error)?;
        let row = fetch(&tx, &id).map_err(OpenError::Db)?;
        tx.commit().map_err(db_error)?;
        row.ok_or_else(|| OpenError::Db("row missing after insert".to_string()))
    }

    pub fn get(&self, id: &str) -> Result<Option<Row>, String> {
        fetch(&self.lock(), id)
    }

    pub fn by_key(&self, key: &str, pid: u32) -> Result<Option<Row>, String> {
        let db = self.lock();
        let rows = select(
            &db,
            "WHERE key = ?1 AND pid = ?2 ORDER BY n LIMIT 1",
            &[&key, &i64::from(pid)],
        )?;
        Ok(rows.into_iter().next())
    }

    pub fn list(&self, session: Option<&str>) -> Result<Vec<Row>, String> {
        let db = self.lock();
        match session {
            Some(session) => select(&db, "WHERE session = ?1 ORDER BY n", &[&session]),
            None => select(&db, "ORDER BY n", &[]),
        }
    }

    pub fn live(&self) -> Result<Vec<Row>, String> {
        let db = self.lock();
        select(&db, "WHERE state IN ('open', 'answered') ORDER BY n", &[])
    }

    pub fn set_found(&self, id: &str, found: &Found, now_ms: i64) -> Result<bool, String> {
        self.lock()
            .execute(
                "UPDATE asks SET asker = ?2, agent_id = ?3, label = ?4, agent_type = ?5,
                     model = ?6, tool_use_id = ?7, updated_ms = ?8
                 WHERE id = ?1",
                params![
                    id,
                    found.asker.as_str(),
                    found.agent_id,
                    found.label,
                    found.agent_type,
                    found.model,
                    found.tool_use_id,
                    now_ms
                ],
            )
            .map(|changed| changed > 0)
            .map_err(db_err)
    }

    pub fn answer(
        &self,
        id: &str,
        answer: &str,
        choice: Option<u8>,
        by: By,
        now_ms: i64,
    ) -> Result<bool, String> {
        self.lock()
            .execute(
                "UPDATE asks SET state = 'answered', answer = ?2, choice = ?3, by = ?4,
                     updated_ms = ?5
                 WHERE id = ?1 AND state = 'open'",
                params![id, answer, choice.map(i64::from), by.as_str(), now_ms],
            )
            .map(|changed| changed > 0)
            .map_err(db_err)
    }

    pub fn close(
        &self,
        id: &str,
        from: &[AskState],
        to: AskState,
        now_ms: i64,
    ) -> Result<bool, String> {
        if from.is_empty() {
            return Ok(false);
        }
        let closed = if to.is_closed() { Some(now_ms) } else { None };
        let sql = format!(
            "UPDATE asks SET state = ?2, updated_ms = ?3, closed_ms = COALESCE(?4, closed_ms)
             WHERE id = ?1 AND state IN ({})",
            states(from)
        );
        self.lock()
            .execute(&sql, params![id, to.as_str(), now_ms, closed])
            .map(|changed| changed > 0)
            .map_err(db_err)
    }

    pub fn set_inbox(&self, id: &str, inbox_id: Option<&str>) -> Result<(), String> {
        self.lock()
            .execute(
                "UPDATE asks SET inbox_id = ?2 WHERE id = ?1",
                params![id, inbox_id],
            )
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn rekey(&self, from: &str, to: &str) -> Result<usize, String> {
        self.lock()
            .execute(
                "UPDATE asks SET session = ?2 WHERE session = ?1",
                params![from, to],
            )
            .map_err(db_err)
    }

    pub fn sweep(&self, now_ms: i64) -> Result<Vec<Row>, String> {
        let cutoff = now_ms.saturating_sub(KEEP_CLOSED_MS);
        let tail = format!(
            "WHERE state IN ({}) AND closed_ms IS NOT NULL AND closed_ms <= ?1 ORDER BY n",
            states(CLOSED)
        );
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let rows = select(&tx, &tail, &[&cutoff])?;
        if !rows.is_empty() {
            tx.execute(
                &format!(
                    "DELETE FROM asks WHERE state IN ({}) AND closed_ms IS NOT NULL
                       AND closed_ms <= ?1",
                    states(CLOSED)
                ),
                [cutoff],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(rows)
    }
}
