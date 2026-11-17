use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;
use crate::transcript::sent::Seen;
use std::sync::{Arc, Barrier};

fn new(session: &str, text: &str) -> New {
    New {
        session: session.to_string(),
        tool: Tool::Claude,
        text: text.to_string(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    }
}

#[test]
fn rows_survive_reopen() {
    let tmp = TempDir::new("outbox-reopen");
    let dir = tmp.path("data");
    let (a, b) = {
        let store = OutboxStore::open(&dir).unwrap();
        let mut first = new("s1", "one");
        first.options = Some(serde_json::json!({"model": "m", "effort": "high"}));
        first.tool = Tool::Codex;
        first.via = Via::Phone;
        first.clear_box = true;
        let a = store.insert(&first, 10).unwrap();
        let b = store.insert(&new("s1", "two"), 20).unwrap();
        (a, b)
    };
    let store = OutboxStore::open(&dir).unwrap();
    assert!(a.clear_box);
    assert!(!b.clear_box);
    assert_eq!(store.list(None).unwrap(), vec![a, b]);
    assert_eq!(store.list(Some("s2")).unwrap(), vec![]);
}

#[test]
fn insert_refuses_empty_too_long_and_full() {
    let store = OutboxStore::in_memory_for_tests();
    assert_eq!(store.insert(&new("s", ""), 1), Err(InsertError::Empty));
    assert_eq!(store.insert(&new("s", "   "), 1), Err(InsertError::Empty));
    let long = "a".repeat(MAX_TEXT + 1);
    assert_eq!(store.insert(&new("s", &long), 1), Err(InsertError::TooLong));
    let exact = "a".repeat(MAX_TEXT);
    store.insert(&new("t", &exact), 1).unwrap();
    for i in 0..MAX_ROWS {
        store.insert(&new("s", &format!("m{i}")), 1).unwrap();
    }
    assert_eq!(store.insert(&new("s", "more"), 1), Err(InsertError::Full));
    store.insert(&new("other", "fits"), 1).unwrap();
}

#[test]
fn command_kind_only_for_claude_slash() {
    let store = OutboxStore::in_memory_for_tests();
    let command = store.insert(&new("s", "/model opus"), 1).unwrap();
    assert_eq!(command.kind, Kind::Command);
    let spaced = store.insert(&new("s", "  /model opus"), 1).unwrap();
    assert_eq!(spaced.kind, Kind::Command);
    let mut codex = new("c", "/model opus");
    codex.tool = Tool::Codex;
    assert_eq!(store.insert(&codex, 1).unwrap().kind, Kind::Prompt);
    let prose = store.insert(&new("s", "hi /x"), 1).unwrap();
    assert_eq!(prose.kind, Kind::Prompt);
}

#[test]
fn new_id_is_o_and_32_lowercase_hex() {
    let id = new_id();
    assert_eq!(id.len(), 33);
    assert!(id.starts_with('o'));
    assert!(id[1..]
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_ne!(id, new_id());
}

#[test]
fn take_only_queued_or_waiting_and_not_before_next_ms() {
    let store = OutboxStore::in_memory_for_tests();
    let boxed = store.insert(&new("s", "a"), 1).unwrap();
    assert!(store
        .set_state(
            &boxed.id,
            &[State::Queued],
            State::Box,
            Some(Why::Mixed),
            None,
            2
        )
        .unwrap());
    assert_eq!(store.take(&boxed.id, 100).unwrap(), None);

    let waiting = store.insert(&new("s", "b"), 1).unwrap();
    assert!(store
        .set_state(
            &waiting.id,
            &[State::Queued],
            State::Waiting,
            Some(Why::Retry),
            Some(51),
            2
        )
        .unwrap());
    assert_eq!(store.take(&waiting.id, 50).unwrap(), None);
    let taken = store.take(&waiting.id, 51).unwrap().unwrap();
    assert_eq!(taken.state, State::Sending);
    assert_eq!(taken.tries, 1);
    assert_eq!(taken.attempt_ms, Some(51));
    assert_eq!(taken.why, None);
    assert_eq!(taken.seen_before, None);
    assert_eq!(store.take(&waiting.id, 60).unwrap(), None);
}

#[test]
fn take_and_edit_race_one_wins() {
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    for round in 0..1000 {
        let row = store.insert(&new("s", "original"), round).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let (s1, g1, id1) = (store.clone(), gate.clone(), row.id.clone());
        let (s2, g2, id2) = (store.clone(), gate.clone(), row.id.clone());
        let taker = std::thread::spawn(move || {
            g1.wait();
            s1.take(&id1, round + 1).unwrap()
        });
        let editor = std::thread::spawn(move || {
            g2.wait();
            s2.edit(&id2, "changed", round + 1).unwrap()
        });
        let taken = taker
            .join()
            .unwrap()
            .expect("a queued row is always takeable");
        let stored = store.list(Some("s")).unwrap().remove(0);
        match editor.join().unwrap() {
            Change::Busy => assert_eq!(taken.text, "original"),
            Change::Done => assert_eq!(taken.text, "changed"),
            Change::Missing | Change::Stale => panic!("round {round}: row vanished"),
        }
        assert_eq!(stored.text, taken.text, "round {round}");
        assert_eq!(stored.state, State::Sending);
        store.delete(&row.id).unwrap();
    }
}

#[test]
fn edit_and_remove_refuse_sending() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    store.take(&row.id, 2).unwrap().unwrap();
    assert_eq!(store.edit(&row.id, "b", 3).unwrap(), Change::Busy);
    assert_eq!(store.remove(&row.id).unwrap(), (Change::Busy, None));
    let kept = store.list(None).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].text, "a");
    assert_eq!(kept[0].state, State::Sending);
    assert_eq!(store.edit("nope", "b", 3).unwrap(), Change::Missing);
    assert_eq!(store.remove("nope").unwrap(), (Change::Missing, None));
}

#[test]
fn edit_resets_the_row_and_rederives_kind() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "plain"), 1).unwrap();
    store
        .set_state(
            &row.id,
            &[State::Queued],
            State::Waiting,
            Some(Why::Retry),
            Some(9),
            4,
        )
        .unwrap();
    store.take(&row.id, 10).unwrap().unwrap();
    store
        .set_state(
            &row.id,
            &[State::Sending],
            State::Stopped,
            None,
            Some(99),
            11,
        )
        .unwrap();
    store.requeue(&row.id, &[State::Stopped], true, 11).unwrap();
    store
        .set_state(
            &row.id,
            &[State::Queued],
            State::Box,
            Some(Why::NotCleared),
            Some(5),
            11,
        )
        .unwrap();
    assert_eq!(store.edit(&row.id, "/model x", 12).unwrap(), Change::Done);
    let edited = store.list(None).unwrap().remove(0);
    assert_eq!(edited.state, State::Queued);
    assert_eq!(edited.kind, Kind::Command);
    assert_eq!(edited.text, "/model x");
    assert_eq!(edited.tries, 0);
    assert_eq!(edited.next_ms, None);
    assert_eq!(edited.why, None);
    assert!(!edited.clear_box);
    assert_eq!(edited.updated_ms, 12);
}

#[test]
fn edit_refuses_unsure_remove_drops_it() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    store.take(&row.id, 2).unwrap().unwrap();
    assert!(store
        .set_state(
            &row.id,
            &[State::Sending],
            State::Unsure,
            Some(Why::NotFound),
            None,
            3
        )
        .unwrap());
    assert_eq!(store.edit(&row.id, "b", 4).unwrap(), Change::Busy);
    let (change, dropped) = store.remove(&row.id).unwrap();
    assert_eq!(change, Change::Done);
    assert_eq!(dropped.unwrap().text, "a");
    assert_eq!(store.list(None).unwrap(), vec![]);
}

#[test]
fn edit_and_remove_refuse_in_queue() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    store.take(&row.id, 2).unwrap().unwrap();
    assert!(store
        .set_state(&row.id, &[State::Sending], State::InQueue, None, None, 3)
        .unwrap());
    let before = store.list(None).unwrap();
    assert_eq!(store.edit(&row.id, "b", 4).unwrap(), Change::Busy);
    assert_eq!(store.remove(&row.id).unwrap(), (Change::Busy, None));
    assert_eq!(store.list(None).unwrap(), before);
    assert_eq!(before[0].state, State::InQueue);
    assert_eq!(before[0].text, "a");
}

#[test]
fn set_state_only_from_the_listed_states() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    assert!(!store
        .set_state(&row.id, &[State::Sending], State::InQueue, None, None, 2)
        .unwrap());
    assert_eq!(store.list(None).unwrap()[0].state, State::Queued);
}

#[test]
fn set_seen_stores_both_baselines() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    store
        .set_seen(
            &row.id,
            Seen {
                enqueued: 2,
                taken: 1,
            },
        )
        .unwrap();
    let still = store.list(None).unwrap().remove(0);
    assert_eq!((still.seen_before, still.queued_before), (None, None));
    store.take(&row.id, 2).unwrap().unwrap();
    store
        .set_seen(
            &row.id,
            Seen {
                enqueued: 2,
                taken: 1,
            },
        )
        .unwrap();
    let now = store.list(None).unwrap().remove(0);
    assert_eq!(now.queued_before, Some(2));
    assert_eq!(now.seen_before, Some(1));
}

#[test]
fn requeue_clears_the_try() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    store.take(&row.id, 2).unwrap().unwrap();
    store
        .set_seen(
            &row.id,
            Seen {
                enqueued: 1,
                taken: 1,
            },
        )
        .unwrap();
    store
        .set_state(
            &row.id,
            &[State::Sending],
            State::Unsure,
            Some(Why::Restart),
            Some(7),
            3,
        )
        .unwrap();
    assert_eq!(
        store.requeue(&row.id, &[State::Box], false, 4).unwrap(),
        Change::Busy
    );
    assert_eq!(
        store.requeue(&row.id, &[State::Unsure], true, 4).unwrap(),
        Change::Done
    );
    let now = store.list(None).unwrap().remove(0);
    assert_eq!(now.state, State::Queued);
    assert!(now.clear_box);
    assert_eq!(now.why, None);
    assert_eq!(now.attempt_ms, None);
    assert_eq!(now.seen_before, None);
    assert_eq!(now.queued_before, None);
    assert_eq!(now.next_ms, None);
    assert_eq!(
        store.requeue("nope", &[State::Unsure], false, 4).unwrap(),
        Change::Missing
    );
}

#[test]
fn untake_gives_the_take_back_with_the_old_tries() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    let taken = store.take(&row.id, 2).unwrap().unwrap();
    assert_eq!(taken.tries, 1);
    assert_eq!(store.untake(&row.id, true, 0, 3).unwrap(), Change::Done);
    let now = store.list(None).unwrap().remove(0);
    assert_eq!(
        (now.state, now.tries, now.clear_box),
        (State::Queued, 0, true)
    );
    assert_eq!(now.attempt_ms, None);
    assert_eq!(store.untake(&row.id, false, 0, 4).unwrap(), Change::Busy);
    assert_eq!(store.untake("nope", false, 0, 4).unwrap(), Change::Missing);
}

#[test]
fn head_and_sessions_follow_order() {
    let store = OutboxStore::in_memory_for_tests();
    let a1 = store.insert(&new("a", "1"), 1).unwrap();
    let b1 = store.insert(&new("b", "2"), 2).unwrap();
    store.insert(&new("a", "3"), 3).unwrap();
    assert_eq!(store.head("a").unwrap().unwrap().id, a1.id);
    assert_eq!(store.head("b").unwrap().unwrap().id, b1.id);
    assert_eq!(store.head("c").unwrap(), None);
    assert_eq!(store.sessions().unwrap(), vec!["a", "b"]);
    assert!(store.delete(&a1.id).unwrap());
    assert!(!store.delete(&a1.id).unwrap());
    assert_eq!(store.head("a").unwrap().unwrap().text, "3");
}

#[test]
fn discard_keeps_sending_row() {
    let store = OutboxStore::in_memory_for_tests();
    let sending = store.insert(&new("s", "a"), 1).unwrap();
    let in_queue = store.insert(&new("s", "b"), 2).unwrap();
    store.insert(&new("s", "c"), 3).unwrap();
    store.insert(&new("other", "d"), 3).unwrap();
    store.take(&sending.id, 4).unwrap().unwrap();
    store.take(&in_queue.id, 4).unwrap().unwrap();
    store
        .set_state(
            &in_queue.id,
            &[State::Sending],
            State::InQueue,
            None,
            None,
            5,
        )
        .unwrap();
    let gone = store.discard_session("s").unwrap();
    let texts: Vec<_> = gone.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, vec!["b", "c"]);
    let left = store.list(None).unwrap();
    let kept: Vec<_> = left
        .iter()
        .map(|r| (r.session.as_str(), r.text.as_str()))
        .collect();
    assert_eq!(kept, vec![("s", "a"), ("other", "d")]);
}

#[test]
fn discard_rows_removes_only_the_named_rows_and_never_a_sending_one() {
    let store = OutboxStore::in_memory_for_tests();
    let a = store.insert(&new("s", "a"), 1).unwrap();
    let b = store.insert(&new("s", "b"), 2).unwrap();
    let c = store.insert(&new("s", "c"), 3).unwrap();
    let d = store.insert(&new("s", "d"), 4).unwrap();
    store
        .set_state(&b.id, &[State::Queued], State::InQueue, None, None, 5)
        .unwrap();
    store.take(&c.id, 6).unwrap();
    let ids = vec![
        a.id.clone(),
        b.id.clone(),
        c.id.clone(),
        "missing".to_string(),
    ];
    let gone = store.discard_rows(&ids).unwrap();
    let texts: Vec<_> = gone.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, vec!["a", "b"]);
    let left: Vec<_> = store
        .list(None)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(left, vec![c.id, d.id]);
}

#[test]
fn rekey_moves_rows() {
    let store = OutboxStore::in_memory_for_tests();
    store.insert(&new("old", "a"), 1).unwrap();
    store.insert(&new("old", "b"), 2).unwrap();
    store.insert(&new("keep", "c"), 3).unwrap();
    assert_eq!(store.rekey("old", "new").unwrap(), 2);
    assert_eq!(store.list(Some("old")).unwrap(), vec![]);
    assert_eq!(store.list(Some("new")).unwrap().len(), 2);
    assert_eq!(store.list(Some("keep")).unwrap().len(), 1);
}

#[test]
fn unnoticed_before_skips_noticed_rows() {
    let store = OutboxStore::in_memory_for_tests();
    let old = store.insert(&new("s", "old"), 10).unwrap();
    let older = store.insert(&new("s", "older"), 5).unwrap();
    store.insert(&new("s", "young"), 100).unwrap();
    let due = store.unnoticed_before(50).unwrap();
    let ids: Vec<_> = due.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, vec![old.id.clone(), older.id.clone()]);
    store
        .mark_noticed(std::slice::from_ref(&old.id), 60)
        .unwrap();
    let due = store.unnoticed_before(50).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, older.id);
    let marked = store.list(None).unwrap();
    assert_eq!(marked[0].noticed_ms, Some(60));
}

#[test]
fn sending_lists_only_sending_rows() {
    let store = OutboxStore::in_memory_for_tests();
    let a = store.insert(&new("s", "a"), 1).unwrap();
    store.insert(&new("t", "b"), 2).unwrap();
    store.take(&a.id, 3).unwrap().unwrap();
    let rows = store.sending().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, a.id);
}

#[test]
fn migrate_from_v3_keeps_rows() {
    let tmp = TempDir::new("outbox-v3");
    let dir = tmp.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE drafts (row_id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE docs (name TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE inbox (id TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL);
             INSERT INTO drafts VALUES ('r1', 'half a thought', 1);
             INSERT INTO settings VALUES ('k', 'v');
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }
    let outbox = OutboxStore::open(&dir).unwrap();
    assert_eq!(outbox.list(None).unwrap(), vec![]);
    assert_eq!(store::drafts(&dir).unwrap()["r1"], "half a thought");
    assert_eq!(store::settings(&dir).unwrap()["k"], "v");
    let version: i64 = store::open(&dir)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 5);
}

fn corrupt_options(store: &OutboxStore, id: &str) {
    store
        .lock()
        .execute(
            "UPDATE outbox SET options = '{not json' WHERE id = ?1",
            [id],
        )
        .unwrap();
}

fn column<T: rusqlite::types::FromSql>(store: &OutboxStore, name: &str, id: &str) -> T {
    store
        .lock()
        .query_row(
            &format!("SELECT {name} FROM outbox WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn malformed_options_is_a_decode_error() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    corrupt_options(&store, &row.id);
    assert!(store.list(None).is_err());
    assert!(store.head("s").is_err());
}

#[test]
fn failed_read_in_take_leaves_the_row_queued() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    corrupt_options(&store, &row.id);
    assert!(store.take(&row.id, 10).is_err());
    assert_eq!(column::<String>(&store, "state", &row.id), "queued");
    assert_eq!(column::<i64>(&store, "tries", &row.id), 0);
}

#[test]
fn get_finds_one_row_by_id() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "a"), 1).unwrap();
    assert_eq!(store.get(&row.id).unwrap(), Some(row));
    assert_eq!(store.get("nope").unwrap(), None);
}
