use super::kinds::{SECRET_REQUEST, SETTING};
use super::model::{
    new_id, AnswerOn, AppliedBy, Consent, New, Refusal, State, IDEM_MS, KEEP_CLOSED_MS,
    OPENED_MIN_MS,
};
use super::rules;
use crate::store;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const COLUMNS: &str = "id, kind, target, change, facts, parts, parts_allowed, reason, session, \
    tool, verified, root, folder, organization, pid, pid_start, idem_key, state, answer_on, \
    applied_by, created_ms, expires_ms, answered_ms, by, used_ms, used_op_id, result, closed_ms, \
    request, extended";

pub struct ConsentStore {
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

fn json_of(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
    let text: String = r.get(index)?;
    serde_json::from_str(&text).map_err(|_| column_error(index, "json"))
}

fn json_opt(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<Value>> {
    let text: Option<String> = r.get(index)?;
    match text {
        Some(text) => Ok(Some(
            serde_json::from_str(&text).map_err(|_| column_error(index, "json"))?,
        )),
        None => Ok(None),
    }
}

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Consent> {
    let pid: i64 = r.get(14)?;
    let pid_start: Option<i64> = r.get(15)?;
    let state: String = r.get(17)?;
    let answer_on: String = r.get(18)?;
    let applied_by: String = r.get(19)?;
    let kind: String = r.get(1)?;
    let change = json_of(r, 3)?;
    let request = match json_of(r, 28)? {
        Value::Null => legacy_request(&kind, &change),
        request => request,
    };
    Ok(Consent {
        id: r.get(0)?,
        kind,
        target: r.get(2)?,
        change,
        facts: json_of(r, 4)?,
        parts: json_opt(r, 5)?,
        parts_allowed: json_opt(r, 6)?,
        reason: r.get(7)?,
        session: r.get(8)?,
        tool: r.get(9)?,
        verified: r.get::<_, i64>(10)? != 0,
        root: r.get(11)?,
        folder: r.get(12)?,
        organization: r.get(13)?,
        pid: u32::try_from(pid).unwrap_or(0),
        pid_start: pid_start.and_then(|n| u64::try_from(n).ok()),
        idem_key: r.get(16)?,
        state: State::parse(&state).ok_or_else(|| column_error(17, "state"))?,
        answer_on: AnswerOn::parse(&answer_on).ok_or_else(|| column_error(18, "answer_on"))?,
        applied_by: AppliedBy::parse(&applied_by).ok_or_else(|| column_error(19, "applied_by"))?,
        created_ms: r.get(20)?,
        expires_ms: r.get(21)?,
        answered_ms: r.get(22)?,
        by: r.get(23)?,
        used_ms: r.get(24)?,
        used_op_id: r.get(25)?,
        result: json_opt(r, 26)?,
        closed_ms: r.get(27)?,
        request,
        extended: r.get::<_, i64>(29)? != 0,
    })
}

fn legacy_request(kind: &str, change: &Value) -> Value {
    match kind {
        SETTING => serde_json::json!({"op": "settings.set", "key": change.get("key"),
                                      "value": change.get("after")}),
        SECRET_REQUEST => serde_json::json!({"op": "vault.request",
                                             "request": change.get("request")}),
        _ => Value::Null,
    }
}

fn fetch(db: &Connection, id: &str) -> Result<Option<Consent>, String> {
    let sql = format!("SELECT {COLUMNS} FROM consents WHERE id = ?1");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let mut rows = query.query_map([id], row_of).map_err(db_err)?;
    rows.next().transpose().map_err(db_err)
}

fn select(
    db: &Connection,
    tail: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Consent>, String> {
    let sql = format!("SELECT {COLUMNS} FROM consents {tail}");
    let mut query = db.prepare_cached(&sql).map_err(db_err)?;
    let rows = query.query_map(args, row_of).map_err(db_err)?;
    rows.collect::<Result<_, _>>().map_err(db_err)
}

fn text_of(value: &Value) -> String {
    value.to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Opened {
    Fresh(Consent),
    Same(Consent),
}

impl Opened {
    pub fn row(&self) -> &Consent {
        match self {
            Opened::Fresh(row) | Opened::Same(row) => row,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Taken {
    Won(Consent),
    Lost(Consent),
    Gone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Restart {
    pub waiting: Vec<Consent>,
    pub withdrawn: Vec<Consent>,
    pub expired: Vec<Consent>,
}

impl ConsentStore {
    pub fn open(data_dir: &Path) -> Result<ConsentStore, String> {
        Ok(ConsentStore {
            db: Mutex::new(store::open(data_dir)?),
        })
    }

    pub fn in_memory_for_tests() -> ConsentStore {
        let db = Connection::open_in_memory().expect("in-memory database");
        store::migrate(&db).expect("migrate");
        ConsentStore { db: Mutex::new(db) }
    }

    pub fn unavailable() -> ConsentStore {
        let db = Connection::open_in_memory().expect("in-memory database");
        ConsentStore { db: Mutex::new(db) }
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, new: &New, now: i64) -> Result<Opened, Refusal> {
        self.insert_with(new, now, &new_id)
    }

    pub fn insert_with(
        &self,
        new: &New,
        now: i64,
        make_id: &dyn Fn() -> Option<String>,
    ) -> Result<Opened, Refusal> {
        let db_error = |e: rusqlite::Error| Refusal::Db(e.to_string());
        let mut db = self.lock();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        if let Some(key) = &new.idem_key {
            let seen = select(
                &tx,
                "WHERE idem_key = ?1 AND created_ms > ?2 ORDER BY n",
                &[key, &(now - IDEM_MS)],
            )
            .map_err(Refusal::Db)?;
            if let Some(first) = seen.into_iter().find(|r| new.owns_key_of(r)) {
                if first.kind == new.kind
                    && first.target == new.target
                    && first.request == new.request
                {
                    return Ok(Opened::Same(first));
                }
                return Err(Refusal::KeyReused);
            }
        }
        let recent = select(
            &tx,
            "WHERE state IN ('waiting', 'opened') OR created_ms > ?1 OR answered_ms > ?1 ORDER BY n",
            &[&(now - super::model::HOUR_MS.max(super::model::COOLDOWN_MS))],
        )
        .map_err(Refusal::Db)?;
        rules::check(new, now, &recent)?;
        let id =
            make_id().ok_or_else(|| Refusal::Db("no randomness for a request id".to_string()))?;
        tx.execute(
            "INSERT INTO consents (id, kind, target, change, facts, parts, reason, session, tool,
                 verified, root, folder, organization, pid, pid_start, idem_key, state, answer_on,
                 applied_by, created_ms, expires_ms, request)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 'waiting', ?17, ?18, ?19, ?20, ?21)",
            params![
                id,
                new.kind,
                new.target,
                text_of(&new.change),
                text_of(&new.facts),
                new.parts.as_ref().map(text_of),
                new.reason,
                new.session,
                new.tool,
                i64::from(new.verified),
                new.root,
                new.folder,
                new.organization,
                i64::from(new.pid),
                new.pid_start.and_then(|n| i64::try_from(n).ok()),
                new.idem_key,
                new.answer_on.as_str(),
                new.applied_by.as_str(),
                now,
                now + new.expires_in_ms,
                text_of(&new.request),
            ],
        )
        .map_err(db_error)?;
        let row = fetch(&tx, &id).map_err(Refusal::Db)?;
        tx.commit().map_err(db_error)?;
        row.map(Opened::Fresh)
            .ok_or_else(|| Refusal::Db("the request was not saved".into()))
    }

    pub fn get(&self, id: &str) -> Result<Option<Consent>, String> {
        fetch(&self.lock(), id)
    }

    pub fn all(&self) -> Result<Vec<Consent>, String> {
        select(&self.lock(), "ORDER BY n DESC", &[])
    }

    pub fn open_rows(&self) -> Result<Vec<Consent>, String> {
        select(
            &self.lock(),
            "WHERE state IN ('waiting', 'opened', 'applying') ORDER BY n",
            &[],
        )
    }

    pub fn of_session(&self, session: &str) -> Result<Vec<Consent>, String> {
        select(
            &self.lock(),
            "WHERE session = ?1 AND verified = 1 ORDER BY n DESC",
            &[&session],
        )
    }

    pub fn answer(
        &self,
        id: &str,
        to: State,
        by: &str,
        parts_allowed: Option<&Value>,
        now: i64,
    ) -> Result<Taken, String> {
        let db = self.lock();
        let closed = to.closed();
        let changed = db
            .execute(
                "UPDATE consents SET state = ?2, by = ?3, answered_ms = ?4, parts_allowed = ?5,
                     used_ms = CASE WHEN ?2 = 'applying' THEN ?4 ELSE used_ms END,
                     closed_ms = CASE WHEN ?6 THEN ?4 ELSE closed_ms END,
                     reason = CASE WHEN ?6 THEN NULL ELSE reason END
                 WHERE id = ?1 AND state IN ('waiting', 'opened') AND expires_ms > ?4",
                params![
                    id,
                    to.as_str(),
                    by,
                    now,
                    parts_allowed.map(text_of),
                    i64::from(closed)
                ],
            )
            .map_err(db_err)?;
        let row = fetch(&db, id)?;
        Ok(match (changed, row) {
            (1, Some(row)) => Taken::Won(row),
            (_, Some(row)) => Taken::Lost(row),
            (_, None) => Taken::Gone,
        })
    }

    pub fn open_sheet(&self, id: &str, now: i64) -> Result<Option<Consent>, String> {
        let db = self.lock();
        db.execute(
            "UPDATE consents SET state = 'opened',
                 expires_ms = CASE WHEN extended = 0 THEN MAX(expires_ms, ?2 + ?3) ELSE expires_ms END,
                 extended = 1
             WHERE id = ?1 AND state = 'waiting' AND expires_ms > ?2 AND applied_by = 'app'",
            params![id, now, OPENED_MIN_MS],
        )
        .map_err(db_err)?;
        fetch(&db, id)
    }

    pub fn resolve_change(&self, id: &str, change: &Value) -> Result<Option<Consent>, String> {
        let db = self.lock();
        db.execute(
            "UPDATE consents SET change = ?2
             WHERE id = ?1 AND state IN ('waiting', 'opened')
                 AND json_type(change, '$.existing') IS NOT 'true'
                 AND json_type(change, '$.existing') IS NOT 'false'",
            params![id, text_of(change)],
        )
        .map_err(db_err)?;
        fetch(&db, id)
    }

    pub fn used(
        &self,
        id: &str,
        op_id: &str,
        by: &str,
        now: i64,
    ) -> Result<Option<Consent>, String> {
        let db = self.lock();
        let changed = db
            .execute(
                "UPDATE consents SET state = 'applying', by = ?3, answered_ms = COALESCE(answered_ms, ?4),
                     used_ms = ?4, used_op_id = ?2
                 WHERE id = ?1 AND state IN ('waiting', 'opened') AND expires_ms > ?4",
                params![id, op_id, by, now],
            )
            .map_err(db_err)?;
        if changed == 0 {
            return Ok(None);
        }
        fetch(&db, id)
    }

    pub fn finish(
        &self,
        id: &str,
        to: State,
        result: Option<&Value>,
        now: i64,
    ) -> Result<Option<Consent>, String> {
        let db = self.lock();
        let from = fetch(&db, id)?;
        let Some(from) = from else {
            return Ok(None);
        };
        if !from.state.next(to) {
            return Ok(None);
        }
        db.execute(
            "UPDATE consents SET state = ?2, result = COALESCE(?3, result), closed_ms = ?4,
                 reason = NULL, used_ms = CASE WHEN ?2 = 'applied' THEN COALESCE(used_ms, ?4) ELSE used_ms END
             WHERE id = ?1 AND state = ?5",
            params![id, to.as_str(), result.map(text_of), now, from.state.as_str()],
        )
        .map_err(db_err)?;
        fetch(&db, id)
    }

    pub fn expire_due(&self, now: i64) -> Result<Vec<Consent>, String> {
        let db = self.lock();
        let due = select(
            &db,
            "WHERE state IN ('waiting', 'opened') AND expires_ms <= ?1",
            &[&now],
        )?;
        db.execute(
            "UPDATE consents SET state = 'expired', closed_ms = ?1, reason = NULL
             WHERE state IN ('waiting', 'opened') AND expires_ms <= ?1",
            [now],
        )
        .map_err(db_err)?;
        db.execute(
            "DELETE FROM consents WHERE closed_ms IS NOT NULL AND closed_ms < ?1",
            [now - KEEP_CLOSED_MS],
        )
        .map_err(db_err)?;
        due.into_iter()
            .map(|row| fetch(&db, &row.id).map(|r| r.unwrap_or(row)))
            .collect()
    }

    pub fn restart(&self, now: i64) -> Result<Restart, String> {
        let db = self.lock();
        let late = select(
            &db,
            "WHERE state IN ('waiting', 'opened') AND expires_ms <= ?1",
            &[&now],
        )?;
        db.execute(
            "UPDATE consents SET state = 'expired', closed_ms = ?1, reason = NULL
             WHERE state IN ('waiting', 'opened') AND expires_ms <= ?1",
            [now],
        )
        .map_err(db_err)?;
        db.execute(
            "UPDATE consents SET state = 'waiting' WHERE state = 'opened'",
            [],
        )
        .map_err(db_err)?;
        let applying = select(&db, "WHERE state = 'applying'", &[])?;
        db.execute(
            "UPDATE consents SET state = 'withdrawn', closed_ms = ?1, reason = NULL
             WHERE state = 'applying'",
            [now],
        )
        .map_err(db_err)?;
        let fresh = |rows: Vec<Consent>| {
            rows.into_iter()
                .map(|row| fetch(&db, &row.id).map(|r| r.unwrap_or(row)))
                .collect::<Result<Vec<_>, _>>()
        };
        let withdrawn = fresh(applying)?;
        let expired = fresh(late)?;
        let waiting = select(&db, "WHERE state = 'waiting' ORDER BY n", &[])?;
        Ok(Restart {
            waiting,
            withdrawn,
            expired,
        })
    }

    #[cfg(test)]
    pub fn run_sql_for_tests(&self, sql: &str) {
        self.lock().execute_batch(sql).expect("sql");
    }
}
