use super::ledger::db::{self, SCHEMA};
use super::ledger::{Ledger, ROW_DAYS};
use super::ledger_testkit::{utc, DAY, NOW};
use crate::catalog::Catalog;
use crate::guard::policy;
use crate::guard::spool;
use crate::sessions::testutil::TempDir;
use serde_json::json;
use std::sync::Arc;

fn ledger(dir: &TempDir) -> Ledger {
    Ledger::open(
        &dir.path("data/daemon/usage.db"),
        Vec::new(),
        Arc::new(Catalog::built_in()),
    )
    .expect("ledger")
    .with_zone(utc)
    .with_guard_dir(dir.path("data/guard"))
}

fn payload(session: &str, tool: &str, input: serde_json::Value) -> Vec<u8> {
    json!({
        "hook_event_name": "PreToolUse",
        "session_id": session,
        "tool_name": tool,
        "tool_use_id": format!("toolu_{session}"),
        "cwd": "/work/SECRET-FOLDER",
        "tool_input": input
    })
    .to_string()
    .into_bytes()
}

fn queue(dir: &TempDir, at: f64, session: &str, command: &str) {
    let guard = dir.path("data/guard");
    policy::write(&guard, 5).expect("policy");
    crate::guard::record(
        &payload(session, "Bash", json!({"command": command})),
        &guard,
        at,
    );
}

#[test]
fn a_database_from_before_the_guard_gains_its_table_and_keeps_its_rows() {
    let dir = TempDir::new("ledger-migrate");
    let path = dir.path("usage.db");
    {
        let old = rusqlite::Connection::open(&path).expect("open");
        old.execute_batch(
            "CREATE TABLE calls (
               key INTEGER PRIMARY KEY, ts REAL NOT NULL, tool TEXT NOT NULL, session TEXT NOT NULL,
               agent TEXT NOT NULL, sub INTEGER NOT NULL, agent_type TEXT, model TEXT NOT NULL,
               effort TEXT, input INTEGER NOT NULL, output INTEGER NOT NULL,
               cache_read INTEGER NOT NULL, write_5m INTEGER NOT NULL, write_1h INTEGER NOT NULL,
               split INTEGER NOT NULL, cost REAL);
             CREATE TABLE rollups (
               day TEXT NOT NULL, tool TEXT NOT NULL, sub INTEGER NOT NULL, model TEXT NOT NULL,
               calls INTEGER NOT NULL, input INTEGER NOT NULL, output INTEGER NOT NULL,
               cache_read INTEGER NOT NULL, write_5m INTEGER NOT NULL, write_1h INTEGER NOT NULL,
               cost REAL NOT NULL, unpriced INTEGER NOT NULL, PRIMARY KEY (day, tool, sub, model));
             CREATE TABLE files (
               path TEXT PRIMARY KEY, size INTEGER NOT NULL, off INTEGER NOT NULL,
               state TEXT NOT NULL, fp TEXT NOT NULL DEFAULT '');
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO calls VALUES (1, 5.0, 'Claude Code', 's', 'main', 0, NULL, 'm', NULL,
               1, 1, 1, 1, 1, 1, 0.5);
             PRAGMA user_version = 2;",
        )
        .expect("old schema");
    }
    let db = db::open_file(&path).expect("migrated");
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("version");
    assert_eq!(version, SCHEMA);
    assert_eq!(SCHEMA, 3);
    assert!(db
        .prepare("SELECT ts, session, agent, tool, rule, action FROM guard_events")
        .is_ok());
    assert_eq!(db::call_count(&db), 1);
}

#[test]
fn a_database_from_a_newer_notchling_is_refused() {
    let dir = TempDir::new("ledger-newer");
    let path = dir.path("usage.db");
    {
        let db = db::open_file(&path).expect("open");
        db.pragma_update(None, "user_version", SCHEMA + 1)
            .expect("bump");
    }
    assert!(db::open_file(&path).is_err());
}

#[test]
fn queued_events_reach_the_database_and_the_queue_is_emptied() {
    let dir = TempDir::new("ledger-guard");
    let mut ledger = ledger(&dir);
    queue(&dir, NOW - 100.0, "s1", "sleep 60");
    queue(&dir, NOW - 90.0, "s1", "ls -R");
    queue(&dir, NOW - 80.0, "s2", "sleep 120");
    assert_eq!(ledger.take_guard(), 3);
    assert!(!dir.path("data/guard/events.log").exists());
    assert!(!dir.path("data/guard/events.log.take").exists());
    assert_eq!(ledger.take_guard(), 0);
    let report = ledger.guard_since(NOW - DAY, NOW, 1);
    assert_eq!(report.total, 3);
    assert_eq!(report.sessions, 2);
    let rules: Vec<(&str, u64, u64)> = report
        .rules
        .iter()
        .map(|r| (r.rule.as_str(), r.count, r.sessions))
        .collect();
    assert_eq!(rules, vec![("tree", 1, 1), ("poll", 2, 2)]);
    ledger.refresh(NOW, None);
    assert_eq!(ledger.guard_since(NOW - DAY, NOW, 1).total, 3);
}

#[test]
fn a_refresh_picks_up_what_the_hook_queued() {
    let dir = TempDir::new("ledger-guard");
    let mut ledger = ledger(&dir);
    queue(&dir, NOW - 10.0, "s1", "sleep 60");
    ledger.refresh(NOW, None);
    assert_eq!(ledger.guard_since(NOW - DAY, NOW, 1).total, 1);
}

#[test]
fn nothing_but_names_and_ids_is_stored() {
    let dir = TempDir::new("ledger-guard");
    let ledger = ledger(&dir);
    queue(&dir, NOW - 10.0, "s1", "ls -R SECRET-COMMAND");
    let guard = dir.path("data/guard");
    crate::guard::record(
        &payload(
            "s1",
            "Read",
            json!({"file_path": "/SECRET/path.log", "description": "SECRET-DESCRIPTION"}),
        ),
        &guard,
        NOW - 9.0,
    );
    crate::guard::record(
        &payload(
            "s1",
            "Agent",
            json!({"description": "SECRET-DESCRIPTION", "prompt": "SECRET-PROMPT", "model": "opus"}),
        ),
        &guard,
        NOW - 8.0,
    );
    assert!(ledger.take_guard() >= 2);
    let db = &ledger.db;
    let mut stmt = db
        .prepare("SELECT ts, session, agent, tool, rule, action FROM guard_events")
        .expect("select");
    let rows: Vec<(f64, String, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .expect("rows")
        .flatten()
        .collect();
    assert!(rows.len() >= 2);
    for (_, session, agent, tool, rule, action) in &rows {
        assert_eq!(session, "s1");
        assert!(agent.is_empty());
        assert!(["Bash", "Read", "Agent"].contains(&tool.as_str()));
        assert!(crate::guard::Rule::parse(rule).is_some());
        assert!(crate::guard::Action::parse(action).is_some());
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").ok();
    for file in ["usage.db", "usage.db-wal"] {
        let bytes = std::fs::read(dir.path(&format!("data/daemon/{file}"))).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        for secret in ["SECRET", "ls -R", "path.log"] {
            assert!(!text.contains(secret), "{secret} in {file}");
        }
    }
}

#[test]
fn events_are_kept_thirty_days_like_calls() {
    let dir = TempDir::new("ledger-guard");
    let mut ledger = ledger(&dir);
    queue(&dir, NOW - (ROW_DAYS as f64 + 1.0) * DAY, "old", "sleep 60");
    queue(
        &dir,
        NOW - (ROW_DAYS as f64 - 1.0) * DAY,
        "recent",
        "sleep 60",
    );
    assert_eq!(ledger.take_guard(), 2);
    ledger.prune(NOW);
    let all = ledger.guard_since(0.0, NOW, 365);
    assert_eq!(all.total, 1);
    assert_eq!(all.sessions, 1);
}

#[test]
fn the_report_counts_only_its_own_window() {
    let dir = TempDir::new("ledger-guard");
    let ledger = ledger(&dir);
    queue(&dir, NOW - 20.0 * DAY, "far", "sleep 60");
    queue(&dir, NOW - 1.0 * DAY, "near", "sleep 60");
    ledger.take_guard();
    let week = ledger.guard_since(NOW - 7.0 * DAY, NOW, 7);
    assert_eq!((week.total, week.sessions, week.days), (1, 1, 7));
    assert_eq!(ledger.guard_since(NOW - 30.0 * DAY, NOW, 30).total, 2);
}

#[test]
fn rows_are_not_lost_when_the_queue_is_taken_but_the_database_refuses() {
    let dir = TempDir::new("ledger-guard");
    queue(&dir, NOW - 10.0, "s1", "sleep 60");
    let ledger = ledger(&dir);
    ledger
        .db
        .execute_batch("DROP TABLE guard_events")
        .expect("drop");
    assert_eq!(ledger.take_guard(), 0);
    assert!(dir.path("data/guard/events.log.take").exists());
    assert!(spool::take(&dir.path("data/guard")).is_some());
}

#[test]
fn a_ledger_without_a_queue_has_an_empty_section() {
    let ledger = Ledger::in_memory(Vec::new(), Arc::new(Catalog::built_in())).expect("ledger");
    assert_eq!(ledger.take_guard(), 0);
    let report = ledger.guard_since(0.0, NOW, 14);
    assert_eq!(
        (report.total, report.sessions, report.rules.len()),
        (0, 0, 0)
    );
}
