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

fn make_unsure(store: &OutboxStore, session: &str, text: &str, at: i64) -> Row {
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
fn a_row_that_goes_unsure_plans_to_step_aside_after_the_hold() {
    let store = OutboxStore::in_memory_for_tests();
    let row = make_unsure(&store, "s", "one", 1_000);
    assert_eq!(row.aside_at_ms, Some(1_000 + UNSURE_HOLD_MS));
    assert_eq!(row.aside_ms, None);
}

#[test]
fn a_second_unsure_in_a_row_plans_nothing_so_it_holds() {
    let store = OutboxStore::in_memory_for_tests();
    make_unsure(&store, "s", "one", 1_000);
    let second = make_unsure(&store, "s", "two", 2_000);
    assert_eq!(second.aside_at_ms, None);
}

#[test]
fn another_session_has_its_own_streak() {
    let store = OutboxStore::in_memory_for_tests();
    make_unsure(&store, "s", "one", 1_000);
    let other = make_unsure(&store, "t", "two", 2_000);
    assert_eq!(other.aside_at_ms, Some(2_000 + UNSURE_HOLD_MS));
}

#[test]
fn a_row_removed_in_between_ends_the_streak() {
    let store = OutboxStore::in_memory_for_tests();
    make_unsure(&store, "s", "one", 1_000);
    let sent = store.insert(&new("s", "sent in between"), 1_500).unwrap();
    store.delete(&sent.id).unwrap();
    let next = make_unsure(&store, "s", "two", 2_000);
    assert_eq!(next.aside_at_ms, Some(2_000 + UNSURE_HOLD_MS));
}

#[test]
fn a_late_proof_or_a_removal_of_the_unsure_row_ends_the_streak_too() {
    let store = OutboxStore::in_memory_for_tests();
    let one = make_unsure(&store, "s", "one", 1_000);
    assert_eq!(store.delete_unsure(&one.id, 1_000), Ok(true));
    let two = make_unsure(&store, "s", "two", 2_000);
    assert!(two.aside_at_ms.is_some());
    store.remove(&two.id).unwrap();
    let three = make_unsure(&store, "s", "three", 3_000);
    assert!(three.aside_at_ms.is_some());
}

#[test]
fn the_same_row_going_unsure_again_is_not_a_second_in_a_row() {
    let store = OutboxStore::in_memory_for_tests();
    let one = make_unsure(&store, "s", "one", 1_000);
    store
        .requeue(&one.id, &[State::Unsure], false, 1_500)
        .unwrap();
    assert_eq!(store.get(&one.id).unwrap().unwrap().aside_at_ms, None);
    store
        .set_state(
            &one.id,
            &[State::Queued],
            State::Unsure,
            Some(Why::NotFound),
            None,
            2_000,
        )
        .unwrap();
    let again = store.get(&one.id).unwrap().unwrap();
    assert_eq!(again.aside_at_ms, Some(2_000 + UNSURE_HOLD_MS));
}

#[test]
fn stepping_aside_needs_the_plan_and_its_time() {
    let store = OutboxStore::in_memory_for_tests();
    let row = make_unsure(&store, "s", "one", 1_000);
    let due = 1_000 + UNSURE_HOLD_MS;
    assert_eq!(store.step_aside(&row.id, due - 1), Ok(false));
    assert_eq!(store.step_aside(&row.id, due), Ok(true));
    assert_eq!(store.step_aside(&row.id, due + 1), Ok(false), "once");
    let aside = store.get(&row.id).unwrap().unwrap();
    assert_eq!(aside.aside_ms, Some(due));
    assert_eq!(aside.state, State::Unsure);
    assert_eq!(aside.updated_ms, 1_000, "the look-again clock is not reset");
}

#[test]
fn a_held_for_good_row_never_steps_aside() {
    let store = OutboxStore::in_memory_for_tests();
    make_unsure(&store, "s", "one", 1_000);
    let second = make_unsure(&store, "s", "two", 2_000);
    assert_eq!(store.step_aside(&second.id, i64::MAX), Ok(false));
}

#[test]
fn the_head_skips_rows_that_stepped_aside() {
    let store = OutboxStore::in_memory_for_tests();
    let one = make_unsure(&store, "s", "one", 1_000);
    let two = store.insert(&new("s", "two"), 2_000).unwrap();
    assert_eq!(store.head("s").unwrap().unwrap().id, one.id);
    store.step_aside(&one.id, 1_000 + UNSURE_HOLD_MS).unwrap();
    assert_eq!(store.head("s").unwrap().unwrap().id, two.id);
    assert_eq!(store.aside("s").unwrap().len(), 1);
    store.delete(&two.id).unwrap();
    assert!(store.head("s").unwrap().is_none());
    assert_eq!(store.list(Some("s")).unwrap().len(), 1);
}

#[test]
fn retry_brings_an_aside_row_back_as_an_ordinary_queued_row() {
    let store = OutboxStore::in_memory_for_tests();
    let one = make_unsure(&store, "s", "one", 1_000);
    store.step_aside(&one.id, 1_000 + UNSURE_HOLD_MS).unwrap();
    store
        .requeue(&one.id, &[State::Unsure], false, 200_000)
        .unwrap();
    let back = store.get(&one.id).unwrap().unwrap();
    assert_eq!(
        (back.state, back.aside_at_ms, back.aside_ms),
        (State::Queued, None, None)
    );
    assert_eq!(store.head("s").unwrap().unwrap().id, one.id);
}

#[test]
fn rows_saved_before_the_hold_existed_hold_as_they_always_did() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "old"), 1).unwrap();
    store.run_sql_for_tests(&format!(
        "UPDATE outbox SET state = 'unsure', why = 'not_found', aside_at_ms = NULL, aside_ms = NULL WHERE id = '{}'",
        row.id
    ));
    assert_eq!(store.step_aside(&row.id, i64::MAX), Ok(false));
    assert_eq!(store.head("s").unwrap().unwrap().id, row.id);
}

#[test]
fn remove_when_only_removes_a_row_still_in_the_state_the_person_saw() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "one"), 1).unwrap();
    let (change, gone) = store.remove_when(&row.id, Some(State::Unsure)).unwrap();
    assert_eq!((change, gone), (Change::Stale, None));
    assert!(store.get(&row.id).unwrap().is_some(), "a queued row stays");
    let unsure = make_unsure(&store, "s", "two", 5);
    let (change, gone) = store.remove_when(&unsure.id, Some(State::Unsure)).unwrap();
    assert_eq!(change, Change::Done);
    assert_eq!(gone.map(|row| row.id), Some(unsure.id));
    assert_eq!(store.remove_when("nope", None).unwrap().0, Change::Missing);
}
