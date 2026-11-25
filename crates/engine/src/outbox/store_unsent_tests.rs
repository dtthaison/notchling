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

fn unsent(store: &OutboxStore, id: &str) -> bool {
    store.get(id).unwrap().unwrap().unsent
}

fn ignored(store: &OutboxStore, text: &str) -> String {
    let id = store.insert(&new(text), 1).unwrap().id;
    store.take(&id, 2).unwrap().unwrap();
    assert!(store.set_unsent(&id, true).unwrap());
    let (from, to) = (&[State::Sending][..], State::Waiting);
    assert!(store
        .set_state(&id, from, to, Some(Why::Retry), Some(3), 3)
        .unwrap());
    id
}

#[test]
fn the_mark_stays_while_the_row_waits_or_sends_and_goes_when_it_settles() {
    let store = OutboxStore::in_memory_for_tests();
    let id = ignored(&store, "hi");
    assert!(unsent(&store, &id));
    let from = &[State::Waiting][..];
    store
        .set_state(&id, from, State::Waiting, Some(Why::Turn), None, 4)
        .unwrap();
    assert!(unsent(&store, &id), "a gate that parks the row keeps it");
    store.take(&id, 5).unwrap().unwrap();
    assert!(unsent(&store, &id), "the retry under way keeps it");
    store
        .set_state(
            &id,
            &[State::Sending],
            State::Unsure,
            Some(Why::NotFound),
            None,
            6,
        )
        .unwrap();
    assert!(!unsent(&store, &id), "Unsure drops it");
    for settled in [State::InQueue, State::Box, State::Stopped, State::Gone] {
        let id = ignored(&store, "again");
        store
            .set_state(&id, &[State::Waiting], settled, None, None, 7)
            .unwrap();
        assert!(!unsent(&store, &id), "{settled:?} drops it");
    }
}

#[test]
fn a_persons_resend_or_edit_drops_the_mark_and_a_try_put_back_keeps_it() {
    let store = OutboxStore::in_memory_for_tests();
    let id = ignored(&store, "hi");
    store.restart(&id, &[State::Waiting], false, 4).unwrap();
    assert!(!unsent(&store, &id));
    let id = ignored(&store, "two");
    store.requeue(&id, &[State::Waiting], false, 4).unwrap();
    assert!(!unsent(&store, &id));
    let id = ignored(&store, "three");
    store.edit(&id, "three, changed", 4).unwrap();
    assert!(!unsent(&store, &id));
    let id = ignored(&store, "four");
    store.take(&id, 5).unwrap().unwrap();
    store.untake(&id, false, 1, 6).unwrap();
    assert!(unsent(&store, &id));
}

#[test]
fn a_settled_row_cannot_be_marked() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    store.take(&id, 2).unwrap().unwrap();
    store
        .set_state(
            &id,
            &[State::Sending],
            State::Unsure,
            Some(Why::NotFound),
            None,
            3,
        )
        .unwrap();
    assert!(!store.set_unsent(&id, true).unwrap());
    assert!(!unsent(&store, &id));
}

#[test]
fn a_database_from_before_the_mark_gets_the_column_and_keeps_its_rows() {
    let tmp = TempDir::new("outbox-v10");
    let dir = tmp.path("data");
    let id = OutboxStore::open(&dir)
        .unwrap()
        .insert(&new("kept"), 1)
        .unwrap()
        .id;
    {
        let db = rusqlite::Connection::open(store::db_path(&dir)).unwrap();
        db.execute_batch("ALTER TABLE outbox DROP COLUMN unsent; PRAGMA user_version = 10;")
            .unwrap();
    }
    let outbox = OutboxStore::open(&dir).unwrap();
    let row = outbox.get(&id).unwrap().unwrap();
    assert_eq!((row.text.as_str(), row.unsent), ("kept", false));
    let version: i64 = store::open(&dir)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 11);
}
