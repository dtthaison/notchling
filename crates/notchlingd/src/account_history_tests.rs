use super::DbSwitches;
use notchling_engine::activity::{ActivityRow, EventsDb};
use notchling_engine::cost::account_log::AccountSwitches;
use std::sync::{Arc, Mutex};

fn switched(seq: u64, ts: i64, provider: &str, to: &str, outcome: &str) -> ActivityRow {
    let mut row = ActivityRow::blank(seq, ts, "account.switched");
    row.outcome = outcome.to_owned();
    row.f = serde_json::json!({"provider": provider, "to": to, "cause": "user"}).to_string();
    row
}

#[test]
fn before_the_log_is_open_there_is_nothing() {
    assert!(DbSwitches::new().switches().is_empty());
}

#[test]
fn the_rows_come_back_oldest_first_for_claude_only() {
    let mut db = EventsDb::memory().unwrap();
    db.insert(&[
        switched(2, 2_000, "claude", "b", "ok"),
        switched(1, 1_000, "claude", "a", "ok"),
        switched(3, 3_000, "codex", "x", "ok"),
        switched(4, 4_000, "claude", "c", "failed"),
        ActivityRow::blank(5, 5_000, "session.reply"),
    ])
    .unwrap();
    let history = DbSwitches::new();
    history.set(Arc::new(Mutex::new(db)));
    let entries = history.switches();
    let seen: Vec<(f64, &str)> = entries.iter().map(|e| (e.at, e.id.as_str())).collect();
    assert_eq!(seen, [(1.0, "a"), (2.0, "b")]);
}
