use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;
use serde_json::json;

pub(super) fn new(session: &str, target: &str) -> New {
    New {
        kind: SETTING.to_string(),
        target: target.to_string(),
        change: json!({"key": target, "before": "a", "after": "b"}),
        facts: json!([{"line": "consent-setting-change", "args": {"key": target}}]),
        parts: None,
        reason: Some("because".to_string()),
        session: Some(session.to_string()),
        tool: "claude".to_string(),
        verified: true,
        root: Some("Terminal".to_string()),
        folder: None,
        organization: None,
        pid: 7,
        pid_start: Some(70),
        idem_key: None,
        answer_on: AnswerOn::MacOrPhone,
        applied_by: AppliedBy::Daemon,
        expires_in_ms: DEFAULT_EXPIRY_MS,
    }
}

#[test]
fn a_request_is_saved_waiting_and_read_back() {
    let store = ConsentStore::in_memory_for_tests();
    let row = store.insert(&new("s1", "k1"), 1_000).unwrap();
    let row = row.row();
    assert!(row.id.starts_with('c') && row.id.len() == 33);
    assert_eq!(row.state, State::Waiting);
    assert_eq!(row.expires_ms, 1_000 + DEFAULT_EXPIRY_MS);
    assert_eq!(row.reason.as_deref(), Some("because"));
    assert_eq!(store.get(&row.id).unwrap().unwrap(), *row);
}

#[test]
fn the_first_answer_wins_and_a_deny_clears_the_reason() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store
        .insert(&new("s1", "k1"), 1_000)
        .unwrap()
        .row()
        .id
        .clone();
    let first = store
        .answer(&id, State::Denied, "phone", None, 2_000)
        .unwrap();
    let Taken::Won(row) = first else {
        panic!("first answer should win")
    };
    assert_eq!(
        (row.state, row.by.as_deref()),
        (State::Denied, Some("phone"))
    );
    assert_eq!(
        row.reason, None,
        "the agent's text goes when the request closes"
    );
    let second = store
        .answer(&id, State::Applying, "mac", None, 3_000)
        .unwrap();
    assert!(matches!(second, Taken::Lost(r) if r.state == State::Denied));
}

#[test]
fn an_answer_after_the_end_is_refused() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    let late = store
        .answer(&id, State::Applying, "mac", None, DEFAULT_EXPIRY_MS + 1)
        .unwrap();
    assert!(matches!(late, Taken::Lost(r) if r.state == State::Waiting));
    let due = store.expire_due(DEFAULT_EXPIRY_MS + 1).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].state, State::Expired);
    assert_eq!(due[0].reason, None);
}

#[test]
fn opening_the_sheet_moves_the_end_to_ten_minutes_out_once() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    let now = DEFAULT_EXPIRY_MS - 60_000;
    let row = store.open_sheet(&id, now).unwrap().unwrap();
    assert_eq!(row.state, State::Opened);
    assert_eq!(row.expires_ms, now + OPENED_MIN_MS);
    let again = store.open_sheet(&id, now + 1_000).unwrap().unwrap();
    assert_eq!(again.expires_ms, now + OPENED_MIN_MS, "moved once");
}

#[test]
fn a_restart_keeps_waiting_returns_opened_to_waiting_and_withdraws_applying() {
    let store = ConsentStore::in_memory_for_tests();
    let waiting = store.insert(&new("s1", "a"), 1).unwrap().row().id.clone();
    let opened = store.insert(&new("s1", "b"), 2).unwrap().row().id.clone();
    let applying = store.insert(&new("s1", "c"), 3).unwrap().row().id.clone();
    store.open_sheet(&opened, 4).unwrap();
    store
        .answer(&applying, State::Applying, "mac", None, 5)
        .unwrap();
    let (live, withdrawn) = store.restart(10).unwrap();
    let ids: Vec<&str> = live.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [waiting.as_str(), opened.as_str()]);
    assert!(live.iter().all(|r| r.state == State::Waiting));
    assert_eq!(withdrawn.len(), 1);
    assert_eq!(withdrawn[0].state, State::Withdrawn);
}

#[test]
fn the_same_key_returns_the_same_request_in_any_state_and_another_value_is_refused() {
    let store = ConsentStore::in_memory_for_tests();
    let mut asked = new("s1", "k1");
    asked.idem_key = Some("key-1".to_string());
    let first = store.insert(&asked, 1_000).unwrap();
    let id = first.row().id.clone();
    store
        .answer(&id, State::Denied, "mac", None, 2_000)
        .unwrap();
    let again = store.insert(&asked, 3_000).unwrap();
    assert!(matches!(&again, Opened::Same(r) if r.id == id && r.state == State::Denied));
    let mut other = asked.clone();
    other.change = json!({"key": "k1", "before": "a", "after": "z"});
    assert_eq!(store.insert(&other, 3_000), Err(Refusal::KeyReused));
    let mut elsewhere = asked.clone();
    elsewhere.session = Some("s2".to_string());
    assert!(matches!(
        store.insert(&elsewhere, 3_000).unwrap(),
        Opened::Fresh(_)
    ));
    assert!(matches!(
        store.insert(&asked, 1_000 + IDEM_MS + 1).unwrap(),
        Opened::Fresh(_)
    ));
}

#[test]
fn a_closed_request_is_deleted_a_week_after_it_closed() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    store.answer(&id, State::Denied, "mac", None, 1).unwrap();
    store.expire_due(KEEP_DAYS_MS - 1).unwrap();
    assert!(store.get(&id).unwrap().is_some());
    store.expire_due(KEEP_DAYS_MS + 2).unwrap();
    assert!(store.get(&id).unwrap().is_none());
}

const KEEP_DAYS_MS: i64 = 7 * 24 * 3_600_000;

#[test]
fn a_database_from_before_the_table_gets_it_and_an_older_build_reads_the_newer_one() {
    let tmp = TempDir::new("consent-old");
    let dir = tmp.root().to_path_buf();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('kept', 'yes');
             PRAGMA user_version = 9;",
        )
        .unwrap();
    }
    let store = ConsentStore::open(&dir).unwrap();
    let id = store.insert(&new("s1", "k1"), 1).unwrap().row().id.clone();
    drop(store);
    let db = store::open(&dir).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 10);
    assert_eq!(store::meta(&db, "kept").unwrap().as_deref(), Some("yes"));
    drop(db);
    let reopened = ConsentStore::open(&dir).unwrap();
    assert!(reopened.get(&id).unwrap().is_some());
    assert_eq!(
        crate::store_class::table_class("consents"),
        Some(crate::store_class::Class::Node)
    );
}
