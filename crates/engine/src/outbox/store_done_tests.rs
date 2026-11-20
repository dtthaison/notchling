use super::*;
use crate::sessions::testutil::TempDir;
use crate::store;

fn new(text: &str) -> New {
    New {
        session: "s".to_string(),
        tool: Tool::Claude,
        text: text.to_string(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    }
}

fn done_from(store: &OutboxStore, id: &str) -> Option<u32> {
    store.get(id).unwrap().unwrap().done_from
}

fn tries(store: &OutboxStore, id: &str) -> u32 {
    store.get(id).unwrap().unwrap().tries
}

#[test]
fn the_done_count_stays_on_the_row_and_only_a_persons_action_starts_it_again() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    assert_eq!(store.count_done(&id, true).unwrap(), Some(0));
    store.take(&id, 2).unwrap().unwrap();
    assert_eq!(store.count_done(&id, true).unwrap(), Some(0));
    assert_eq!(store.get(&id).unwrap().unwrap().updated_ms, 2);
    store.untake(&id, false, 1, 3).unwrap();
    assert_eq!(
        done_from(&store, &id),
        Some(0),
        "a try put back is not a person's"
    );

    store.take(&id, 4).unwrap().unwrap();
    store.requeue(&id, &[State::Sending], false, 5).unwrap();
    assert_eq!((done_from(&store, &id), tries(&store, &id)), (None, 2));

    assert_eq!(store.count_done(&id, true).unwrap(), Some(2));
    store.take(&id, 6).unwrap().unwrap();
    store.restart(&id, &[State::Sending], false, 7).unwrap();
    assert_eq!((done_from(&store, &id), tries(&store, &id)), (None, 0));

    assert_eq!(store.count_done(&id, true).unwrap(), Some(0));
    assert_eq!(store.edit(&id, "again", 8).unwrap(), Change::Done);
    assert_eq!(done_from(&store, &id), None);

    assert_eq!(store.count_done(&id, true).unwrap(), Some(0));
    assert_eq!(store.count_done(&id, false).unwrap(), None);
    store.run_sql_for_tests(&format!(
        "UPDATE outbox SET done_from = 7 WHERE id = '{id}'"
    ));
    assert_eq!(
        store.count_done(&id, true).unwrap(),
        Some(0),
        "a start above the tries is taken again from the tries"
    );
}

const OLDER_COLUMNS: &str = "n, id, session, tool, kind, text, options, via, state, why, \
    clear_box, queued_ms, updated_ms, attempt_ms, seen_before, queued_before, tries, next_ms, \
    noticed_ms, anchor_file, anchor_slack_ms, aside_at_ms, aside_ms, hold_ms, joined, awaited";

#[test]
fn a_counted_row_is_read_and_changed_by_an_older_daemon() {
    let tmp = TempDir::new("outbox-done-old");
    let dir = tmp.path("data");
    let outbox = OutboxStore::open(&dir).unwrap();
    let id = outbox.insert(&new("hi"), 1).unwrap().id;
    outbox.count_done(&id, true).unwrap();
    outbox.insert(&new("next"), 2).unwrap();
    let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert!(version > 8, "an older daemon's migrate finds nothing to do");
    let sql = format!("SELECT {OLDER_COLUMNS} FROM outbox ORDER BY n");
    let mut query = db.prepare(&sql).unwrap();
    let read: Vec<(String, Option<String>)> = query
        .query_map([], |r| Ok((r.get(8)?, r.get(9)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(read.len(), 2);
    for (state, why) in &read {
        assert!(State::parse(state).is_some(), "{state}");
        assert!(why.as_deref().is_none_or(|w| Why::parse(w).is_some()));
    }
    db.execute(
        "INSERT INTO outbox (id, session, tool, kind, text, via, state, queued_ms, updated_ms)
         VALUES ('older', 's', 'claude', 'prompt', 'from an older daemon', 'chat', 'queued', 3, 3)",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE outbox SET state = 'queued', why = NULL, tries = 0, updated_ms = 4 WHERE id = ?1",
        [&id],
    )
    .unwrap();
    let rows = OutboxStore::open(&dir).unwrap().list(None).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter().find(|r| r.id == "older").unwrap().done_from,
        None
    );
}

#[test]
fn a_database_from_before_the_count_gets_the_column_and_keeps_its_rows() {
    let tmp = TempDir::new("outbox-v8");
    let dir = tmp.path("data");
    let id = OutboxStore::open(&dir)
        .unwrap()
        .insert(&new("kept"), 1)
        .unwrap()
        .id;
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch("ALTER TABLE outbox DROP COLUMN done_from; PRAGMA user_version = 8;")
            .unwrap();
    }
    let outbox = OutboxStore::open(&dir).unwrap();
    let row = outbox.get(&id).unwrap().unwrap();
    assert_eq!((row.text.as_str(), row.done_from), ("kept", None));
    assert_eq!(outbox.count_done(&id, true).unwrap(), Some(0));
    let version: i64 = store::open(&dir)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 9);
}
