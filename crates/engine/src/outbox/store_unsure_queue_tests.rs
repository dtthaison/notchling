use super::*;

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

fn go_unsure(store: &OutboxStore, session: &str, text: &str, at: i64) -> Row {
    let row = store.insert(&new(session, text), 1).unwrap();
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
fn a_planned_unsure_row_steps_aside_now_without_waiting_for_its_time() {
    let store = OutboxStore::in_memory_for_tests();
    let row = go_unsure(&store, "s", "is it right", 5_000);
    assert_eq!(row.aside_at_ms, Some(5_000 + UNSURE_HOLD_MS));
    assert_eq!(store.step_aside(&row.id, 6_000), Ok(false));
    assert_eq!(store.step_aside_now(&row.id, 6_000), Ok(true));
    let kept = store.get(&row.id).unwrap().unwrap();
    assert_eq!(kept.state, State::Unsure);
    assert_eq!(kept.aside_ms, Some(6_000));
    assert_eq!(store.step_aside_now(&row.id, 7_000), Ok(false));
}

#[test]
fn a_second_unsure_in_a_row_never_steps_aside_early() {
    let store = OutboxStore::in_memory_for_tests();
    go_unsure(&store, "s", "one", 1_000);
    let second = go_unsure(&store, "s", "two", 2_000);
    assert_eq!(second.aside_at_ms, None);
    assert_eq!(store.step_aside_now(&second.id, 3_000), Ok(false));
    assert_eq!(store.get(&second.id).unwrap().unwrap().aside_ms, None);
}

#[test]
fn a_row_that_is_not_unsure_is_left_alone() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "queued"), 1).unwrap();
    assert_eq!(store.step_aside_now(&row.id, 2_000), Ok(false));
}

#[test]
fn the_head_after_an_early_aside_is_the_row_queued_behind() {
    let store = OutboxStore::in_memory_for_tests();
    let first = go_unsure(&store, "s", "is it right", 5_000);
    let second = store.insert(&new("s", "add id"), 5_100).unwrap();
    assert_eq!(store.head("s").unwrap().unwrap().id, first.id);
    assert_eq!(store.step_aside_now(&first.id, 5_200), Ok(true));
    assert_eq!(store.head("s").unwrap().unwrap().id, second.id);
    assert_eq!(store.aside("s").unwrap().len(), 1);
}

#[test]
fn a_queued_row_is_removed_only_by_a_join_that_keeps_its_text_or_by_the_person() {
    let store = OutboxStore::in_memory_for_tests();
    let first = go_unsure(&store, "s", "is it right", 5_000);
    store.step_aside_now(&first.id, 5_000).unwrap();
    let a = store.insert(&new("s", "add id"), 5_100).unwrap();
    let b = store.insert(&new("s", "to the lists"), 5_200).unwrap();
    let c = store.insert(&new("s", "done"), 5_300).unwrap();
    store.take(&a.id, 6_000).unwrap().unwrap();
    let joined = store.join_behind(&a.id, 6_000).unwrap();
    let ids: Vec<&str> = joined.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, [b.id.as_str(), c.id.as_str()]);
    let carrier = store.get(&a.id).unwrap().unwrap();
    for text in ["add id", "to the lists", "done"] {
        assert!(carrier.text.contains(text), "{text} is in {}", carrier.text);
    }
    assert_eq!(carrier.joined, 3);
    assert!(store.get(&first.id).unwrap().is_some());
}
