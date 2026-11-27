use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;
use std::sync::{Arc, Barrier};

fn new(key: &str, session: &str) -> New {
    New {
        key: key.to_string(),
        session: session.to_string(),
        session_id: None,
        pid: 100,
        pid_start: None,
        folder: None,
        branch: None,
        question: "Keep it?".to_string(),
        choices: None,
        main_may_answer: false,
        mode: Mode::Background,
        deadline_ms: None,
    }
}

fn found() -> Found {
    Found {
        asker: Asker::Subagent,
        agent_id: Some("a1".to_string()),
        label: Some("Review".to_string()),
        agent_type: Some("general".to_string()),
        model: Some("sonnet".to_string()),
        tool_use_id: Some("toolu_1".to_string()),
    }
}

const HOUR: i64 = 3_600_000;

#[test]
fn rows_survive_reopen() {
    let tmp = TempDir::new("ask-reopen");
    let dir = tmp.path("data");
    let (a, b) = {
        let store = AskStore::open(&dir).unwrap();
        let mut first = new("k1", "s1");
        first.session_id = Some("sid".to_string());
        first.pid = 4242;
        first.pid_start = Some(1_700_000_000_123);
        first.folder = Some("/work/repo".to_string());
        first.branch = Some("agent/x".to_string());
        first.question = "Naïve «q» 日本語?".to_string();
        first.choices = Some(vec!["Yes".to_string(), "No".to_string()]);
        first.main_may_answer = true;
        first.mode = Mode::Wait;
        first.deadline_ms = Some(99_000);
        let a = store.insert(&first, 10).unwrap();
        let b = store.insert(&new("k2", "s1"), 20).unwrap();
        store.set_found(&a.id, &found(), 11).unwrap();
        store.set_inbox(&a.id, Some("inbox-1")).unwrap();
        (store.get(&a.id).unwrap().unwrap(), b)
    };
    assert_eq!(a.asker, Asker::Subagent);
    assert_eq!(b.asker, Asker::Unknown);
    let store = AskStore::open(&dir).unwrap();
    let rows = store.list(None).unwrap();
    assert_eq!(rows, vec![a.clone(), b]);
    assert_eq!(a.pid_start, Some(1_700_000_000_123));
    assert_eq!(a.choices.as_deref().map(<[String]>::len), Some(2));
    assert_eq!(a.inbox_id.as_deref(), Some("inbox-1"));
    assert_eq!(a.state, AskState::Open);
    assert_eq!(store.list(Some("s2")).unwrap(), vec![]);
    assert_eq!(store.list(Some("s1")).unwrap().len(), 2);
}

#[test]
fn same_key_and_pid_returns_the_same_row() {
    let store = AskStore::in_memory_for_tests();
    let first = store.insert(&new("k", "s"), 1).unwrap();
    let again = store.insert(&new("k", "s"), 99).unwrap();
    assert_eq!(first, again);
    assert_eq!(store.list(None).unwrap().len(), 1);
}

#[test]
fn by_key_finds_a_row_in_any_session_for_that_pid_only() {
    let store = AskStore::in_memory_for_tests();
    let first = store.insert(&new("k", "s"), 1).unwrap();
    store.rekey("s", "elsewhere").unwrap();
    assert_eq!(store.by_key("k", 100).unwrap().unwrap().id, first.id);
    assert_eq!(store.by_key("k", 101).unwrap(), None);
    assert_eq!(store.by_key("other", 100).unwrap(), None);
}

#[test]
fn same_key_other_pid_is_a_new_row() {
    let store = AskStore::in_memory_for_tests();
    let first = store.insert(&new("k", "s"), 1).unwrap();
    let mut other = new("k", "s");
    other.pid = 101;
    let second = store.insert(&other, 2).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(store.list(None).unwrap().len(), 2);
}

#[test]
fn open_counts_per_session_and_in_all() {
    let store = AskStore::in_memory_for_tests();
    let mut ids = Vec::new();
    for i in 0..MAX_OPEN_PER_SESSION {
        ids.push(store.insert(&new(&format!("a{i}"), "s1"), 1).unwrap().id);
    }
    assert_eq!(
        store.insert(&new("a-over", "s1"), 1),
        Err(OpenError::SessionFull)
    );
    store.insert(&new("b0", "s2"), 1).unwrap();
    store.answer(&ids[0], "x", None, By::Mac, 2).unwrap();
    assert_eq!(
        store.insert(&new("a-answered", "s1"), 3),
        Err(OpenError::SessionFull)
    );
    store
        .close(&ids[0], &[AskState::Answered], AskState::Delivered, 4)
        .unwrap();
    store.insert(&new("a-after-close", "s1"), 5).unwrap();

    let all = AskStore::in_memory_for_tests();
    for session in 0..5 {
        for i in 0..MAX_OPEN / 5 {
            all.insert(&new(&format!("k{session}-{i}"), &format!("s{session}")), 1)
                .unwrap();
        }
    }
    assert_eq!(
        all.insert(&new("k-extra", "s5"), 1),
        Err(OpenError::AllFull)
    );
    let first = all.list(None).unwrap().remove(0);
    all.close(&first.id, &[AskState::Open], AskState::Dismissed, 2)
        .unwrap();
    all.insert(&new("k-extra", "s5"), 3).unwrap();
}

#[test]
fn answer_only_from_open() {
    let store = AskStore::in_memory_for_tests();
    let row = store.insert(&new("k", "s"), 1).unwrap();
    assert!(store.answer(&row.id, "Yes", Some(1), By::Phone, 5).unwrap());
    let got = store.get(&row.id).unwrap().unwrap();
    assert_eq!(got.state, AskState::Answered);
    assert_eq!(got.answer.as_deref(), Some("Yes"));
    assert_eq!(got.choice, Some(1));
    assert_eq!(got.by, Some(By::Phone));
    assert_eq!(got.updated_ms, 5);
    assert!(!store.answer(&row.id, "No", Some(2), By::Mac, 6).unwrap());
    assert_eq!(store.get(&row.id).unwrap().unwrap(), got);

    let dismissed = store.insert(&new("k2", "s"), 1).unwrap();
    assert!(store
        .close(&dismissed.id, &[AskState::Open], AskState::Dismissed, 2)
        .unwrap());
    assert!(!store.answer(&dismissed.id, "x", None, By::Mac, 3).unwrap());
    assert_eq!(
        store.get(&dismissed.id).unwrap().unwrap().state,
        AskState::Dismissed
    );
    assert!(!store.answer("missing", "x", None, By::Mac, 3).unwrap());
}

#[test]
fn answer_races_have_one_winner() {
    let tmp = TempDir::new("ask-race");
    let store = Arc::new(AskStore::open(&tmp.path("data")).unwrap());
    let row = store.insert(&new("k", "s"), 1).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = row.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .answer(&id, &format!("a{i}"), None, By::Mac, 2)
                    .unwrap()
            })
        })
        .collect();
    let wins = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(wins, 1);
}

#[test]
fn close_moves_only_from_listed_states() {
    let store = AskStore::in_memory_for_tests();
    let row = store.insert(&new("k", "s"), 1).unwrap();
    assert!(!store
        .close(&row.id, &[AskState::Answered], AskState::Delivered, 2)
        .unwrap());
    assert!(!store.close(&row.id, &[], AskState::Gone, 2).unwrap());
    let still = store.get(&row.id).unwrap().unwrap();
    assert_eq!(still.state, AskState::Open);
    assert_eq!(still.closed_ms, None);

    store.answer(&row.id, "Yes", Some(1), By::Mac, 3).unwrap();
    assert!(store
        .close(
            &row.id,
            &[AskState::Open, AskState::Answered],
            AskState::Delivered,
            7
        )
        .unwrap());
    let done = store.get(&row.id).unwrap().unwrap();
    assert_eq!(done.state, AskState::Delivered);
    assert_eq!(done.answer.as_deref(), Some("Yes"));
    assert_eq!(done.choice, Some(1));
    assert_eq!(done.by, Some(By::Mac));
    assert_eq!(done.closed_ms, Some(7));
    assert_eq!(done.updated_ms, 7);
    assert!(!store
        .close(
            &row.id,
            &[AskState::Open, AskState::Answered],
            AskState::Gone,
            8
        )
        .unwrap());
}

#[test]
fn live_lists_open_and_answered_only() {
    let store = AskStore::in_memory_for_tests();
    let a = store.insert(&new("a", "s"), 1).unwrap();
    let b = store.insert(&new("b", "s"), 1).unwrap();
    let c = store.insert(&new("c", "s"), 1).unwrap();
    store.answer(&b.id, "x", None, By::Mac, 2).unwrap();
    store
        .close(&c.id, &[AskState::Open], AskState::Expired, 2)
        .unwrap();
    let live: Vec<String> = store.live().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(live, vec![a.id, b.id]);
}

#[test]
fn set_found_inbox_and_missing_rows() {
    let store = AskStore::in_memory_for_tests();
    let row = store.insert(&new("k", "s"), 1).unwrap();
    assert!(store.set_found(&row.id, &found(), 9).unwrap());
    assert!(!store.set_found("missing", &found(), 9).unwrap());
    let got = store.get(&row.id).unwrap().unwrap();
    assert_eq!(got.asker, Asker::Subagent);
    assert_eq!(got.agent_id.as_deref(), Some("a1"));
    assert_eq!(got.tool_use_id.as_deref(), Some("toolu_1"));
    assert_eq!(got.updated_ms, 9);
    store.set_inbox(&row.id, Some("i")).unwrap();
    assert_eq!(
        store.get(&row.id).unwrap().unwrap().inbox_id.as_deref(),
        Some("i")
    );
    store.set_inbox(&row.id, None).unwrap();
    assert_eq!(store.get(&row.id).unwrap().unwrap().inbox_id, None);
}

#[test]
fn sweep_deletes_closed_after_a_day_only() {
    let store = AskStore::in_memory_for_tests();
    let now = 100 * 24 * HOUR;
    let old = store.insert(&new("old", "s"), 1).unwrap();
    store
        .answer(&old.id, "secret answer", None, By::Mac, 2)
        .unwrap();
    store
        .close(
            &old.id,
            &[AskState::Answered],
            AskState::Delivered,
            now - 24 * HOUR,
        )
        .unwrap();
    let recent = store.insert(&new("recent", "s"), 1).unwrap();
    store
        .close(
            &recent.id,
            &[AskState::Open],
            AskState::Dismissed,
            now - 23 * HOUR,
        )
        .unwrap();
    let open = store
        .insert(&new("open", "s"), now - 7 * 24 * HOUR)
        .unwrap();
    let swept = store.sweep(now).unwrap();
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].id, old.id);
    assert_eq!(swept[0].answer.as_deref(), Some("secret answer"));
    assert_eq!(store.get(&old.id).unwrap(), None);
    assert!(store.get(&recent.id).unwrap().is_some());
    assert!(store.get(&open.id).unwrap().is_some());
    assert_eq!(store.sweep(now).unwrap(), vec![]);
}

#[test]
fn rekey_moves_rows() {
    let store = AskStore::in_memory_for_tests();
    let a = store.insert(&new("a", "sid"), 1).unwrap();
    let b = store.insert(&new("b", "sid"), 1).unwrap();
    let other = store.insert(&new("c", "other"), 1).unwrap();
    assert_eq!(store.rekey("sid", "job1").unwrap(), 2);
    assert_eq!(store.get(&a.id).unwrap().unwrap().session, "job1");
    assert_eq!(store.get(&b.id).unwrap().unwrap().session, "job1");
    assert_eq!(store.get(&other.id).unwrap().unwrap().session, "other");
    assert_eq!(store.rekey("sid", "job2").unwrap(), 0);
    assert_eq!(store.list(Some("job1")).unwrap().len(), 2);
}

#[test]
fn migrate_from_v4_keeps_outbox_rows() {
    let tmp = TempDir::new("ask-v4");
    let dir = tmp.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE drafts (
                 row_id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE docs (name TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE inbox (
                 id TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE outbox (
                 n INTEGER PRIMARY KEY AUTOINCREMENT,
                 id TEXT NOT NULL UNIQUE,
                 session TEXT NOT NULL,
                 tool TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 text TEXT NOT NULL,
                 options TEXT,
                 via TEXT NOT NULL,
                 state TEXT NOT NULL,
                 why TEXT,
                 clear_box INTEGER NOT NULL DEFAULT 0,
                 queued_ms INTEGER NOT NULL,
                 updated_ms INTEGER NOT NULL,
                 attempt_ms INTEGER,
                 seen_before INTEGER,
                 queued_before INTEGER,
                 tries INTEGER NOT NULL DEFAULT 0,
                 next_ms INTEGER,
                 noticed_ms INTEGER
             );
             CREATE INDEX outbox_session ON outbox(session, n);
             CREATE INDEX outbox_age ON outbox(queued_ms);
             INSERT INTO outbox (id, session, tool, kind, text, via, state, queued_ms, updated_ms)
                 VALUES ('o1', 's', 'claude', 'prompt', 'kept', 'chat', 'queued', 1, 1);
             PRAGMA user_version = 4;",
        )
        .unwrap();
    }
    let asks = AskStore::open(&dir).unwrap();
    assert_eq!(asks.list(None).unwrap(), vec![]);
    asks.insert(&new("k", "s"), 1).unwrap();
    let outbox = crate::outbox::OutboxStore::open(&dir).unwrap();
    let rows = outbox.list(None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "kept");
    let db = store::open(&dir).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 11);
}

#[test]
fn indexes_exist() {
    let tmp = TempDir::new("ask-index");
    let db = store::open(&tmp.path("data")).unwrap();
    let mut query = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'asks'")
        .unwrap();
    let names: Vec<String> = query
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(names.contains(&"asks_session".to_string()), "{names:?}");
    assert!(names.contains(&"asks_state".to_string()), "{names:?}");
}
