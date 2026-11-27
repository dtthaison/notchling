use super::*;
use crate::transcript::sent::Seen;

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

fn read(store: &OutboxStore, id: &str) -> Row {
    store.get(id).unwrap().unwrap()
}

fn retry(store: &OutboxStore, text: &str) -> Row {
    let id = store.insert(&new(text), 1).unwrap().id;
    store.take(&id, 2).unwrap().unwrap();
    let first = Seen {
        taken: 4,
        enqueued: 2,
    };
    store.set_baseline(&id, first, None).unwrap();
    assert!(store.set_unsent(&id, true).unwrap());
    let (from, to) = (&[State::Sending][..], State::Waiting);
    assert!(store
        .set_state(&id, from, to, Some(Why::Retry), Some(3), 3)
        .unwrap());
    read(store, &id)
}

fn waiting_untried(store: &OutboxStore, text: &str) -> Row {
    let id = store.insert(&new(text), 1).unwrap().id;
    let (from, to) = (&[State::Queued][..], State::Waiting);
    assert!(store
        .set_state(&id, from, to, Some(Why::Dialog), None, 2)
        .unwrap());
    read(store, &id)
}

fn left_alone(store: &OutboxStore, seen: &Row, what: &str) -> Row {
    let moved = read(store, &seen.id);
    assert_ne!(&moved, seen, "{what}");
    assert_eq!(store.take_unmoved(seen, 50).unwrap(), None, "{what}");
    assert_eq!(read(store, &seen.id), moved, "{what}");
    moved
}

#[test]
fn a_row_that_is_as_it_was_read_is_taken() {
    let store = OutboxStore::in_memory_for_tests();
    let seen = retry(&store, "hi");
    let taken = store.take_unmoved(&seen, 5).unwrap().unwrap();
    assert_eq!(
        (taken.state, taken.why, taken.tries, taken.seen_before),
        (State::Sending, None, 2, None),
        "{taken:?}"
    );
    assert_eq!((taken.attempt_ms, taken.updated_ms), (Some(5), 5));
    assert_eq!(
        store.take_unmoved(&seen, 6).unwrap(),
        None,
        "a taken row is not taken again"
    );
}

#[test]
fn a_resend_after_the_read_is_not_taken_over() {
    let store = OutboxStore::in_memory_for_tests();
    let seen = retry(&store, "hi");
    let resent = store.requeue(&seen.id, &[State::Waiting], false, 4);
    assert_eq!(resent, Ok(Change::Done));
    let moved = left_alone(&store, &seen, "a resend");
    assert_eq!(
        (moved.state, moved.tries, moved.unsent, moved.seen_before),
        (State::Queued, seen.tries, false, None),
        "{moved:?}"
    );
    let taken = store.take_unmoved(&moved, 51).unwrap().unwrap();
    assert_eq!(taken.tries, 2, "read again, the row is taken");
}

#[test]
fn a_same_text_edit_of_an_untried_row_is_not_taken_over() {
    let store = OutboxStore::in_memory_for_tests();
    let seen = waiting_untried(&store, "hi");
    assert_eq!(store.edit(&seen.id, "hi", 3), Ok(Change::Done));
    let moved = left_alone(&store, &seen, "a same-text edit");
    assert_eq!(
        (moved.state, moved.why, moved.tries, moved.text.as_str()),
        (State::Queued, None, seen.tries, "hi"),
        "{moved:?}"
    );
    assert!(store.take_unmoved(&moved, 51).unwrap().is_some());
}

#[test]
fn a_write_in_the_same_millisecond_is_still_a_move() {
    let store = OutboxStore::in_memory_for_tests();
    let boxed = New {
        clear_box: true,
        ..new("hi")
    };
    let id = store.insert(&boxed, 1).unwrap().id;
    let seen = read(&store, &id);
    assert_eq!(store.edit(&id, "hi", seen.updated_ms), Ok(Change::Done));
    let moved = left_alone(&store, &seen, "an edit in the read's millisecond");
    assert_eq!(
        (moved.state, moved.tries, moved.updated_ms, &moved.text),
        (seen.state, seen.tries, seen.updated_ms, &seen.text),
        "only the box flag tells them apart"
    );
    assert!(seen.clear_box && !moved.clear_box);
}

#[test]
fn a_write_that_keeps_the_rows_time_is_still_a_move() {
    type Write = fn(&OutboxStore, &Row);
    let writes: [(&str, Write); 3] = [
        ("the unsent mark", |store, seen| {
            assert!(store.set_unsent(&seen.id, false).unwrap());
        }),
        ("the done count", |store, seen| {
            store.count_done(seen, true).unwrap();
        }),
        ("the inbox notice", |store, seen| {
            store
                .mark_noticed(std::slice::from_ref(&seen.id), 9)
                .unwrap();
        }),
    ];
    for (what, write) in writes {
        let store = OutboxStore::in_memory_for_tests();
        let seen = retry(&store, "hi");
        write(&store, &seen);
        let moved = left_alone(&store, &seen, what);
        assert_eq!(moved.updated_ms, seen.updated_ms, "{what}");
        assert!(store.take_unmoved(&moved, 51).unwrap().is_some(), "{what}");
    }
}

#[test]
fn a_held_or_removed_row_is_not_taken() {
    let store = OutboxStore::in_memory_for_tests();
    let seen = waiting_untried(&store, "hi");
    assert_eq!(store.hold(&seen.id, Some(90), 3), Ok(Change::Done));
    let held = left_alone(&store, &seen, "a hold");
    assert_eq!(
        store.take_unmoved(&held, 51).unwrap(),
        None,
        "the lease is on"
    );
    assert!(store.take_unmoved(&held, 90).unwrap().is_some());

    let seen = waiting_untried(&store, "there");
    assert_eq!(store.remove(&seen.id).unwrap().0, Change::Done);
    assert_eq!(store.take_unmoved(&seen, 50).unwrap(), None);
    assert_eq!(store.get(&seen.id).unwrap(), None, "it stays removed");
}
