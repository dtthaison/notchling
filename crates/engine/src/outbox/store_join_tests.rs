use super::*;
use serde_json::json;

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

fn head_of(store: &OutboxStore, rows: &[&str]) -> Vec<Row> {
    let made: Vec<Row> = rows
        .iter()
        .enumerate()
        .map(|(i, text)| store.insert(&new("s", text), 10 + i as i64).unwrap())
        .collect();
    store.take(&made[0].id, 100).unwrap().expect("taken");
    made
}

#[test]
fn the_rows_right_behind_the_head_go_with_it_in_order_after_a_blank_line() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two", "three"]);
    let gone = store.join_behind(&made[0].id, 100).unwrap();
    assert_eq!(
        gone.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
        ["two", "three"]
    );
    let head = store.get(&made[0].id).unwrap().unwrap();
    assert_eq!(head.text, "one\n\ntwo\n\nthree");
    assert_eq!(head.joined, 3);
    assert_eq!(head.state, State::Sending);
    assert_eq!(head.queued_ms, 10, "it keeps its place and its time");
    assert_eq!(store.list(Some("s")).unwrap().len(), 1);
}

#[test]
fn a_head_with_nothing_behind_it_stays_one_message() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one"]);
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());
    let head = store.get(&made[0].id).unwrap().unwrap();
    assert_eq!(head.text, "one");
    assert_eq!(head.joined, 0);
}

#[test]
fn waiting_rows_for_a_queue_a_turn_or_a_dialog_are_joined() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two", "three", "four"]);
    for (row, why) in [
        (&made[1], Why::Queue),
        (&made[2], Why::Turn),
        (&made[3], Why::Dialog),
    ] {
        store
            .set_state(
                &row.id,
                &[State::Queued],
                State::Waiting,
                Some(why),
                None,
                50,
            )
            .unwrap();
    }
    let gone = store.join_behind(&made[0].id, 100).unwrap();
    assert_eq!(gone.len(), 3);
    assert_eq!(
        store.get(&made[0].id).unwrap().unwrap().text,
        "one\n\ntwo\n\nthree\n\nfour"
    );
}

#[test]
fn the_join_stops_at_a_row_that_is_not_untouched() {
    for stop in ["retry", "unsure", "box", "stopped", "in_queue", "sending"] {
        let store = OutboxStore::in_memory_for_tests();
        let made = head_of(&store, &["one", "two", "three"]);
        let sql = match stop {
            "retry" => format!(
                "UPDATE outbox SET state = 'waiting', why = 'retry', tries = 1, next_ms = 999999 WHERE id = '{}'",
                made[1].id
            ),
            other => format!("UPDATE outbox SET state = '{other}' WHERE id = '{}'", made[1].id),
        };
        store.run_sql_for_tests(&sql);
        assert!(
            store.join_behind(&made[0].id, 100).unwrap().is_empty(),
            "{stop}"
        );
        assert_eq!(
            store.get(&made[2].id).unwrap().unwrap().text,
            "three",
            "{stop}"
        );
    }
}

#[test]
fn a_command_a_shell_line_other_options_and_clear_box_stop_the_join() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two"]);
    store.run_sql_for_tests(&format!(
        "UPDATE outbox SET kind = 'command', text = '/compact' WHERE id = '{}'",
        made[1].id
    ));
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());

    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two"]);
    store.run_sql_for_tests(&format!(
        "UPDATE outbox SET options = '{}' WHERE id = '{}'",
        json!({"model": "opus"}),
        made[1].id
    ));
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());

    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two"]);
    store.run_sql_for_tests(&format!(
        "UPDATE outbox SET clear_box = 1 WHERE id = '{}'",
        made[1].id
    ));
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());
}

#[test]
fn a_head_that_is_a_command_or_clears_the_box_takes_nothing_with_it() {
    let store = OutboxStore::in_memory_for_tests();
    let command = store.insert(&new("s", "/compact"), 1).unwrap();
    store.insert(&new("s", "after"), 2).unwrap();
    store.take(&command.id, 100).unwrap().unwrap();
    assert!(store.join_behind(&command.id, 100).unwrap().is_empty());

    let mut clearing = new("t", "one");
    clearing.clear_box = true;
    let head = store.insert(&clearing, 1).unwrap();
    store.insert(&new("t", "two"), 2).unwrap();
    store.take(&head.id, 100).unwrap().unwrap();
    assert!(store.join_behind(&head.id, 100).unwrap().is_empty());
}

#[test]
fn a_row_of_another_session_or_tool_is_not_joined() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one"]);
    store
        .insert(&new("elsewhere", "other session"), 20)
        .unwrap();
    let mut codex = new("s", "other tool");
    codex.tool = Tool::Codex;
    store.insert(&codex, 21).unwrap();
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());
}

#[test]
fn the_join_stops_before_the_size_limit() {
    let store = OutboxStore::in_memory_for_tests();
    let big = "x".repeat(MAX_TEXT - 3);
    let made = head_of(&store, &[big.as_str(), "ab", "cd"]);
    let gone = store.join_behind(&made[0].id, 100).unwrap();
    assert!(gone.is_empty(), "{}", gone.len());
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &[big.as_str(), "a", "cd"]);
    let gone = store.join_behind(&made[0].id, 100).unwrap();
    assert_eq!(gone.len(), 1);
    assert_eq!(store.get(&made[2].id).unwrap().unwrap().text, "cd");
}

#[test]
fn a_held_row_is_not_joined_until_its_lease_ends() {
    let store = OutboxStore::in_memory_for_tests();
    let made = head_of(&store, &["one", "two"]);
    store.hold(&made[1].id, Some(500), 100).unwrap();
    assert!(store.join_behind(&made[0].id, 100).unwrap().is_empty());
    assert_eq!(store.join_behind(&made[0].id, 500).unwrap().len(), 1);
}

#[test]
fn only_a_sending_head_joins_rows() {
    let store = OutboxStore::in_memory_for_tests();
    let first = store.insert(&new("s", "one"), 1).unwrap();
    store.insert(&new("s", "two"), 2).unwrap();
    assert!(store.join_behind(&first.id, 100).unwrap().is_empty());
}

#[test]
fn a_hold_is_a_lease_edit_and_requeue_end_and_take_respects() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "one"), 1).unwrap();
    assert_eq!(store.hold(&row.id, Some(900), 10), Ok(Change::Done));
    assert_eq!(store.get(&row.id).unwrap().unwrap().hold_ms, Some(900));
    assert_eq!(store.take(&row.id, 500), Ok(None), "held");
    assert!(store.take(&row.id, 900).unwrap().is_some(), "lease over");
    store
        .requeue(&row.id, &[State::Sending], false, 950)
        .unwrap();
    assert_eq!(store.get(&row.id).unwrap().unwrap().hold_ms, None);

    assert_eq!(store.hold(&row.id, Some(2_000), 1_000), Ok(Change::Done));
    assert_eq!(store.edit(&row.id, "one, better", 1_100), Ok(Change::Done));
    assert_eq!(store.get(&row.id).unwrap().unwrap().hold_ms, None);

    assert_eq!(store.hold(&row.id, Some(2_000), 1_200), Ok(Change::Done));
    assert_eq!(store.hold(&row.id, None, 1_300), Ok(Change::Done));
    assert_eq!(store.get(&row.id).unwrap().unwrap().hold_ms, None);
    assert_eq!(store.hold("missing", Some(1), 1), Ok(Change::Missing));
}

#[test]
fn only_a_row_that_can_still_be_edited_is_held() {
    let store = OutboxStore::in_memory_for_tests();
    let row = store.insert(&new("s", "one"), 1).unwrap();
    store.take(&row.id, 5).unwrap().unwrap();
    assert_eq!(store.hold(&row.id, Some(900), 10), Ok(Change::Busy));
}
