use super::*;
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
fn a_new_message_joins_the_last_queued_row_and_keeps_its_place() {
    let store = OutboxStore::in_memory_for_tests();
    let first = store.insert(&new("s", "one"), 10).unwrap();
    let joined = store
        .combine(&new("s", "two"), 50)
        .unwrap()
        .expect("joined");
    assert_eq!(joined.id, first.id);
    assert_eq!(joined.text, "one\n\ntwo");
    assert_eq!(joined.state, State::Queued);
    assert_eq!(joined.queued_ms, 10);
    assert_eq!(joined.updated_ms, 50);
    assert_eq!(joined.n, first.n);
    let more = store
        .combine(&new("s", "three"), 60)
        .unwrap()
        .expect("joined");
    assert_eq!(more.text, "one\n\ntwo\n\nthree");
    assert_eq!(store.list(Some("s")).unwrap().len(), 1);
}

#[test]
fn only_the_last_row_of_the_same_session_takes_part() {
    let store = OutboxStore::in_memory_for_tests();
    store.insert(&new("other", "elsewhere"), 1).unwrap();
    assert_eq!(store.combine(&new("s", "one"), 5).unwrap(), None);
    let first = store.insert(&new("s", "one"), 10).unwrap();
    let second = store.insert(&new("s", "two"), 11).unwrap();
    let joined = store
        .combine(&new("s", "three"), 12)
        .unwrap()
        .expect("joined");
    assert_eq!(joined.id, second.id);
    assert_eq!(joined.text, "two\n\nthree");
    assert_eq!(store.get(&first.id).unwrap().unwrap().text, "one");
    assert_eq!(store.list(Some("other")).unwrap()[0].text, "elsewhere");
}

#[test]
fn a_row_the_postman_has_is_never_added_to() {
    for state in [
        State::Sending,
        State::InQueue,
        State::Unsure,
        State::Box,
        State::Stopped,
        State::Gone,
        State::Waiting,
    ] {
        let store = OutboxStore::in_memory_for_tests();
        let row = store.insert(&new("s", "one"), 10).unwrap();
        store
            .set_state(&row.id, &[State::Queued], state, None, None, 20)
            .unwrap();
        assert_eq!(
            store.combine(&new("s", "two"), 30).unwrap(),
            None,
            "{state:?}"
        );
        let kept = store.get(&row.id).unwrap().unwrap();
        assert_eq!(
            (kept.text.as_str(), kept.updated_ms),
            ("one", 20),
            "{state:?}"
        );
    }
}

#[test]
fn a_different_tool_options_command_or_clear_box_stays_apart() {
    let store = OutboxStore::in_memory_for_tests();
    store.insert(&new("s", "one"), 10).unwrap();
    let mut codex = new("s", "two");
    codex.tool = Tool::Codex;
    assert_eq!(store.combine(&codex, 20).unwrap(), None);
    let mut model = new("s", "two");
    model.options = Some(serde_json::json!({"model": "m"}));
    assert_eq!(store.combine(&model, 20).unwrap(), None);
    let mut clearing = new("s", "two");
    clearing.clear_box = true;
    assert_eq!(store.combine(&clearing, 20).unwrap(), None);
    assert_eq!(store.combine(&new("s", "/compact"), 20).unwrap(), None);
    assert_eq!(
        store
            .get(&store.list(None).unwrap()[0].id)
            .unwrap()
            .unwrap()
            .text,
        "one"
    );

    let command = store.insert(&new("c", "/compact"), 30).unwrap();
    assert_eq!(store.combine(&new("c", "then this"), 40).unwrap(), None);
    assert_eq!(store.get(&command.id).unwrap().unwrap().text, "/compact");
}

#[test]
fn rows_of_the_same_options_and_a_codex_row_join() {
    let store = OutboxStore::in_memory_for_tests();
    let mut first = new("c", "one");
    first.tool = Tool::Codex;
    first.options = Some(serde_json::json!({"model": "m"}));
    store.insert(&first, 10).unwrap();
    let mut second = first.clone();
    second.text = "two".to_string();
    let joined = store.combine(&second, 20).unwrap().expect("joined");
    assert_eq!(joined.text, "one\n\ntwo");
    assert_eq!(joined.options, first.options);
}

#[test]
fn a_message_that_would_pass_the_limit_makes_a_new_row() {
    let store = OutboxStore::in_memory_for_tests();
    let big = "x".repeat(MAX_TEXT - 10);
    store.insert(&new("s", &big), 10).unwrap();
    assert_eq!(store.combine(&new("s", "twelve chars"), 20).unwrap(), None);
    let edge = store
        .combine(&new("s", "12345678"), 21)
        .unwrap()
        .expect("fits");
    assert_eq!(edge.text.len(), MAX_TEXT);
    assert_eq!(store.combine(&new("s", "y"), 22).unwrap(), None);
}

#[test]
fn a_combine_and_a_take_never_lose_text() {
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    for round in 0..1000 {
        let row = store.insert(&new("s", "one"), round).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let (s1, g1, id1) = (store.clone(), gate.clone(), row.id.clone());
        let (s2, g2) = (store.clone(), gate.clone());
        let taker = std::thread::spawn(move || {
            g1.wait();
            s1.take(&id1, round + 1).unwrap()
        });
        let joiner = std::thread::spawn(move || {
            g2.wait();
            s2.combine(&new("s", "two"), round + 1).unwrap()
        });
        let taken = taker.join().unwrap().expect("a queued row is takeable");
        let joined = joiner.join().unwrap();
        let rows = store.list(Some("s")).unwrap();
        match joined {
            Some(_) => {
                assert_eq!(taken.text, "one\n\ntwo", "round {round}");
                assert_eq!(rows.len(), 1);
            }
            None => {
                assert_eq!(taken.text, "one", "round {round}");
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].state, State::Sending);
            }
        }
        assert_eq!(rows[0].text, taken.text, "round {round}");
        store.delete(&row.id).unwrap();
    }
}

#[test]
fn a_row_the_caller_keeps_apart_is_not_added_to() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "one"), 10).unwrap();
    let kept = row.id.clone();
    let apart = move |id: &str| id == kept;
    assert_eq!(
        store.combine_except(&new("s", "two"), 20, &apart).unwrap(),
        None
    );
    assert_eq!(store.get(&row.id).unwrap().unwrap().text, "one");
    assert!(store
        .combine_except(&new("s", "two"), 20, &|_| false)
        .unwrap()
        .is_some());
}
