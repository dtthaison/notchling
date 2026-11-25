use crate::guard::spool::Taken;
use crate::guard::Rule;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RuleCount {
    pub rule: String,
    pub count: u64,
    pub sessions: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GuardReport {
    pub days: u32,
    pub total: u64,
    pub sessions: u64,
    pub rules: Vec<RuleCount>,
}

pub fn insert_all(db: &Connection, taken: &Taken) -> rusqlite::Result<usize> {
    let tx = db.unchecked_transaction()?;
    let mut added = 0;
    {
        let mut claim =
            tx.prepare("INSERT OR IGNORE INTO guard_batches (id, ts) VALUES (?1,?2)")?;
        let mut stmt = tx.prepare(
            "INSERT INTO guard_events (ts, session, agent, tool, rule, action)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        for batch in &taken.batches {
            let Some(first) = batch.rows.first() else {
                continue;
            };
            if claim.execute(params![batch.id, first.ts])? == 0 {
                continue;
            }
            for row in &batch.rows {
                stmt.execute(params![
                    row.ts,
                    row.session,
                    row.agent,
                    row.tool,
                    row.rule,
                    row.action
                ])?;
                added += 1;
            }
        }
    }
    tx.commit()?;
    Ok(added)
}

pub fn summary(db: &Connection, from: f64, to: f64, days: u32) -> GuardReport {
    let mut report = GuardReport {
        days,
        ..GuardReport::default()
    };
    let Ok(mut stmt) = db.prepare(
        "SELECT rule, COUNT(*), COUNT(DISTINCT session) FROM guard_events
         WHERE ts >= ?1 AND ts <= ?2 GROUP BY rule",
    ) else {
        return report;
    };
    let rows = stmt.query_map(params![from, to], |r| {
        Ok(RuleCount {
            rule: r.get(0)?,
            count: r.get::<_, i64>(1)? as u64,
            sessions: r.get::<_, i64>(2)? as u64,
        })
    });
    if let Ok(rows) = rows {
        report.rules = rows.flatten().collect();
    }
    let order = |rule: &str| Rule::parse(rule).map_or(usize::MAX, |r| r as usize);
    report.rules.sort_by_key(|r| order(&r.rule));
    report.total = report.rules.iter().map(|r| r.count).sum();
    report.sessions = db
        .query_row(
            "SELECT COUNT(DISTINCT session) FROM guard_events WHERE ts >= ?1 AND ts <= ?2",
            params![from, to],
            |r| r.get::<_, i64>(0),
        )
        .map_or(0, |n| n as u64);
    report
}
