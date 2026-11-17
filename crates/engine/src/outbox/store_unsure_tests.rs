use super::*;

fn new(text: &str) -> New {
    New {
        session: "s1".to_string(),
        tool: Tool::Claude,
        text: text.to_string(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    }
}

fn unsure(store: &OutboxStore, text: &str, at: i64) -> Row {
    let row = store.insert(&new(text), 1).unwrap();
    store
        .set_state(
            &row.id,
            &[State::Queued],
            State::Unsure,
            Some(Why::NotFound),
            None,
            at,
        )
        .unwrap();
    store.get(&row.id).unwrap().unwrap()
}

#[test]
fn an_unsure_row_is_deleted_only_as_it_was_seen() {
    let store = OutboxStore::in_memory_for_tests();
    let row = unsure(&store, "one", 30);
    assert_eq!(store.delete_unsure(&row.id, 29), Ok(false));
    assert!(store.get(&row.id).unwrap().is_some());
    assert_eq!(store.delete_unsure(&row.id, 30), Ok(true));
    assert!(store.get(&row.id).unwrap().is_none());
    assert_eq!(store.delete_unsure(&row.id, 30), Ok(false));
}

#[test]
fn a_row_that_left_unsure_is_not_deleted_as_unsure() {
    let store = OutboxStore::in_memory_for_tests();
    let row = unsure(&store, "one", 30);
    store
        .set_state(&row.id, &[State::Unsure], State::Queued, None, None, 30)
        .unwrap();
    assert_eq!(store.delete_unsure(&row.id, 30), Ok(false));
    assert_eq!(store.get(&row.id).unwrap().unwrap().state, State::Queued);
}
