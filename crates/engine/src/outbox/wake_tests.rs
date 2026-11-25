use super::*;
use crate::reply::exec::Failure;

const NOTHING: Tried = Tried {
    pasted: false,
    entered: false,
};

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

#[test]
fn only_a_timeout_before_any_key_is_forgiven() {
    let timed_out = TryEnd::Failed(Failure::Timeout);
    assert!(forgiven(&timed_out, NOTHING));
    let entered = Tried {
        pasted: true,
        entered: true,
    };
    assert!(!forgiven(&timed_out, entered));
    for other in [
        Failure::NoInput,
        Failure::NotSent,
        Failure::NotTaken,
        Failure::NotCleared,
        Failure::Mixed,
        Failure::Dialog,
    ] {
        assert!(!forgiven(&TryEnd::Failed(other), NOTHING));
    }
    assert!(!forgiven(&TryEnd::AttachFailed, NOTHING));
    assert!(!forgiven(&TryEnd::Ok, NOTHING));
}

fn done_from(store: &OutboxStore, id: &str) -> Option<u32> {
    store.get(id).unwrap().unwrap().done_from
}

#[test]
fn a_forgiven_try_moves_the_done_count_up_once_and_only_while_the_row_is_sending() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    let seen = store.get(&id).unwrap().unwrap();
    assert_eq!(store.count_done(&seen, true).unwrap(), Some(0));
    assert!(
        !store.forgive_wake(&id).unwrap(),
        "a queued row has no try to forgive"
    );

    let taken = store.take(&id, 2).unwrap().unwrap();
    assert_eq!((taken.tries, taken.done_from), (1, Some(0)));
    assert!(store.forgive_wake(&id).unwrap());
    assert_eq!(done_from(&store, &id), Some(1));
    assert!(
        !store.forgive_wake(&id).unwrap(),
        "one try is forgiven once"
    );

    let moved = store
        .set_state(
            &id,
            &[State::Sending],
            State::Waiting,
            Some(Why::Retry),
            Some(3),
            3,
        )
        .unwrap();
    assert!(moved);
    assert!(!store.forgive_wake(&id).unwrap());
    assert_eq!(done_from(&store, &id), Some(1));
}

#[test]
fn a_row_with_no_done_count_has_nothing_to_forgive() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    store.take(&id, 2).unwrap().unwrap();
    assert!(!store.forgive_wake(&id).unwrap());
    assert_eq!(done_from(&store, &id), None);
}

#[test]
fn a_timeout_after_the_paste_is_never_forgiven() {
    let timed_out = TryEnd::Failed(Failure::Timeout);
    let pasted = Tried {
        pasted: true,
        entered: false,
    };
    assert!(!forgiven(&timed_out, pasted));
}

fn wake_once(store: &OutboxStore, id: &str, now: i64) -> bool {
    store.take(id, now).unwrap().unwrap();
    let forgiven = store.forgive_wake(id).unwrap();
    let moved = store
        .set_state(
            id,
            &[State::Sending],
            State::Waiting,
            Some(Why::Retry),
            Some(now),
            now,
        )
        .unwrap();
    assert!(moved);
    forgiven
}

#[test]
fn forgiven_wakes_are_counted_for_each_done_episode_not_by_the_rows_lifetime_tries() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    store.run_sql_for_tests(&format!("UPDATE outbox SET tries = 9 WHERE id = '{id}'"));
    let seen = store.get(&id).unwrap().unwrap();
    assert_eq!(store.count_done(&seen, true).unwrap(), Some(9));
    for n in 1..=WAKE_FORGIVEN_TRIES {
        assert!(wake_once(&store, &id, 100 * i64::from(n)), "wake {n}");
    }
    assert!(
        !wake_once(&store, &id, 10_000),
        "the next wake counts again"
    );
}

#[test]
fn a_new_done_episode_gets_its_forgiven_wakes_again() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    let seen = store.get(&id).unwrap().unwrap();
    store.count_done(&seen, true).unwrap();
    for n in 1..=WAKE_FORGIVEN_TRIES {
        assert!(wake_once(&store, &id, 100 * i64::from(n)), "wake {n}");
    }
    assert!(!wake_once(&store, &id, 10_000));
    let live = store.get(&id).unwrap().unwrap();
    assert_eq!(store.count_done(&live, false).unwrap(), None);
    let again = store.get(&id).unwrap().unwrap();
    assert!(store.count_done(&again, true).unwrap().is_some());
    assert!(wake_once(&store, &id, 20_000), "a new episode");
}

#[test]
fn a_put_back_wake_keeps_the_episodes_spent_wakes_and_counted_tries() {
    let store = OutboxStore::in_memory_for_tests();
    let id = store.insert(&new("hi"), 1).unwrap().id;
    let seen = store.get(&id).unwrap().unwrap();
    store.count_done(&seen, true).unwrap();
    for n in 1..=WAKE_FORGIVEN_TRIES {
        assert!(wake_once(&store, &id, 100 * i64::from(n)), "wake {n}");
    }
    assert!(!wake_once(&store, &id, 5_000), "a counted try");
    let before = store.get(&id).unwrap().unwrap();
    store.take(&id, 6_000).unwrap().unwrap();
    assert_eq!(store.put_back(&before, false, 6_001).unwrap(), Change::Done);
    let back = store.get(&id).unwrap().unwrap();
    let from = Some(WAKE_FORGIVEN_TRIES);
    assert_eq!(store.count_done(&back, true).unwrap(), from);
    assert!(!wake_once(&store, &id, 7_000), "the wakes stay spent");
}
