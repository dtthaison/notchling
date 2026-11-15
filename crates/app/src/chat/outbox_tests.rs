use super::*;
use crate::chat::outgoing::GRACE_MS;
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_local_api::Event;
use serde_json::{json, Value};

fn wire(id: &str, session: &str, state: &str, why: Option<&str>, text: &str) -> Value {
    let mut row = json!({
        "id": id, "session": session, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "position": 1, "queuedMs": 10_000, "updatedMs": 10_000, "via": "chat",
    });
    if let Some(why) = why {
        row["why"] = json!(why);
    }
    row
}

fn row(id: &str, state: &str, why: Option<&str>, text: &str) -> OutboxRow {
    OutboxRow::from_wire(&wire(id, "ab12cd34", state, why, text)).expect("a row")
}

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.into(),
        data,
    }
}

fn changed(row: Value) -> Event {
    event("outbox.changed", json!({"row": row}))
}

fn removed(id: &str, session: &str, why: &str) -> Event {
    event(
        "outbox.removed",
        json!({"id": id, "session": session, "why": why}),
    )
}

fn user_items(texts: &[(&str, f64)]) -> Conversation {
    Conversation {
        items: texts
            .iter()
            .enumerate()
            .map(|(i, (t, at))| ChatItem {
                id: format!("i{i}"),
                kind: ItemKind::User {
                    text: (*t).to_owned(),
                    images: 0,
                },
                date: Some(*at),
            })
            .collect(),
        ..Conversation::default()
    }
}

#[test]
fn every_state_maps_to_words_and_buttons() {
    use Button::*;
    let cases: Vec<(OutboxRow, Status, Vec<Button>)> = vec![
        (
            row("a", "queued", None, "x"),
            Status::Queued,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("queue"), "x"),
            Status::Queued,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("retry"), "x"),
            Status::QueuedRetry,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("dialog"), "x"),
            Status::WaitingDialog,
            vec![Edit, Remove],
        ),
        (
            row("a", "waiting", Some("turn"), "x"),
            Status::WaitsForTurn,
            vec![Edit, Remove],
        ),
        (row("a", "sending", None, "x"), Status::Sending, vec![]),
        (
            row("a", "in_queue", None, "x"),
            Status::QueuedInSession,
            vec![],
        ),
        (
            row("a", "unsure", Some("not_found"), "x"),
            Status::MayBeIn,
            vec![SendAgain, Drop],
        ),
        (
            row("a", "stopped", None, "x"),
            Status::Stopped,
            vec![ResumeAndSend, Edit, Remove],
        ),
        (row("a", "gone", None, "x"), Status::NotFound, vec![Remove]),
    ];
    for (row, status, buttons) in cases {
        let shown = bubble(&row);
        assert_eq!(shown.status, status, "{:?} {:?}", row.state, row.why);
        assert_eq!(shown.buttons, buttons, "{:?} {:?}", row.state, row.why);
        assert_eq!(shown.text, "x");
        assert_eq!(shown.id, "a");
    }
}

#[test]
fn a_box_row_shows_the_boxes_text_on_one_line_with_its_three_buttons() {
    let mut value = wire("b", "ab12cd34", "box", Some("not_cleared"), "mine");
    value["box"] = json!("my own\ndraft  here");
    let row = OutboxRow::from_wire(&value).unwrap();
    let shown = bubble(&row);
    assert_eq!(shown.status, Status::BoxHasText("my own draft here".into()));
    assert_eq!(
        shown.buttons,
        vec![Button::ClearBoxAndSend, Button::Edit, Button::Remove]
    );
    let bare = OutboxRow::from_wire(&wire("c", "s", "box", Some("mixed"), "t")).unwrap();
    assert_eq!(bubble(&bare).status, Status::BoxHasText(String::new()));
}

#[test]
fn a_row_with_a_word_this_app_does_not_know_is_skipped() {
    assert!(OutboxRow::from_wire(&wire("a", "s", "teleporting", None, "x")).is_none());
    assert!(OutboxRow::from_wire(&json!({"id": "a"})).is_none());
}

#[test]
fn buttons_that_act_without_text_make_their_request() {
    assert_eq!(
        Button::Remove.request("o1").unwrap().args,
        json!({"id": "o1"})
    );
    assert_eq!(Button::Drop.request("o1").unwrap().op, "outbox.remove");
    let again = Button::SendAgain.request("o1").unwrap();
    assert_eq!(
        (again.op, again.args),
        ("outbox.resend", json!({"id": "o1"}))
    );
    assert_eq!(
        Button::ClearBoxAndSend.request("o1").unwrap().args,
        json!({"id": "o1", "clear_box": true})
    );
    assert_eq!(
        Button::ResumeAndSend.request("o1").unwrap().args,
        json!({"id": "o1", "resume": true})
    );
    assert!(Button::Edit.request("o1").is_none());
}

#[test]
fn in_queue_row_hides_matching_conversation_queued_entry() {
    let queued = vec![
        "first".to_owned(),
        "ok  then".to_owned(),
        "ok then".to_owned(),
    ];
    let held = [row("a", "in_queue", None, "ok then")];
    assert_eq!(hidden_queued(&queued, &held), Some(1));
    let waiting = [row("a", "queued", None, "ok then")];
    assert_eq!(hidden_queued(&queued, &waiting), None);
    let other = [row("a", "in_queue", None, "something else")];
    assert_eq!(hidden_queued(&queued, &other), None);
    assert_eq!(hidden_queued(&[], &held), None);
}

#[test]
fn changed_and_removed_apply_in_order() {
    let mut feed = OutboxFeed::default();
    assert!(feed.apply(&changed(wire("a", "s1", "queued", None, "one")), 0));
    assert!(feed.apply(&changed(wire("b", "s1", "queued", None, "two")), 0));
    assert!(feed.apply(&changed(wire("c", "s2", "queued", None, "other")), 0));
    let ids = |feed: &OutboxFeed, s: &str| -> Vec<String> {
        feed.rows(s).iter().map(|r| r.id.clone()).collect()
    };
    assert_eq!(ids(&feed, "s1"), ["a", "b"]);
    assert_eq!(ids(&feed, "s2"), ["c"]);
    assert!(feed.apply(&changed(wire("a", "s1", "sending", None, "one")), 0));
    assert_eq!(feed.rows("s1")[0].state, State::Sending);
    assert_eq!(feed.rows("s1").len(), 2);
    assert!(feed.apply(&removed("a", "s1", "removed"), 0));
    assert_eq!(ids(&feed, "s1"), ["b"]);
    assert!(!feed.apply(&removed("zzz", "s1", "removed"), 0));
    assert!(feed.rows("nobody").is_empty());
    assert!(!feed.apply(&event("outbox.other", json!({})), 0));
}

#[test]
fn reconnect_reseeds_from_list() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("old", "s1", "queued", None, "gone")), 0);
    feed.seed(&[
        wire("n1", "s1", "queued", None, "one"),
        wire("n2", "s2", "waiting", Some("dialog"), "two"),
    ]);
    assert_eq!(feed.rows("s1").len(), 1);
    assert_eq!(feed.rows("s1")[0].id, "n1");
    assert_eq!(feed.rows("s2")[0].why, Some(Why::Dialog));
}

#[test]
fn sent_bubble_stays_until_transcript_shows_it() {
    let mut feed = OutboxFeed::default();
    feed.apply(
        &changed(wire("a", "s1", "sending", None, "ship it")),
        10_000,
    );
    assert!(feed.apply(&removed("a", "s1", "sent"), 12_000));
    assert!(feed.rows("s1").is_empty());
    let shown = feed.bubbles("s1");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::Sent);
    assert!(shown[0].buttons.is_empty());

    let before = user_items(&[("something older", 1.0)]);
    feed.settle("s1", &before, 13_000);
    assert_eq!(
        feed.bubbles("s1").len(),
        1,
        "the transcript has no copy yet"
    );

    let after = user_items(&[("something older", 1.0), ("ship it\n", 12.5)]);
    feed.settle("s1", &after, 14_000);
    assert!(feed.bubbles("s1").is_empty());
}

#[test]
fn an_older_identical_message_does_not_end_the_sent_bubble() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "ok")), 100_000);
    feed.apply(&removed("a", "s1", "sent"), 102_000);
    let older = user_items(&[("ok", 5.0)]);
    feed.settle("s1", &older, 103_000);
    assert_eq!(feed.bubbles("s1").len(), 1);
}

#[test]
fn a_sent_bubble_the_transcript_never_shows_lets_go_after_the_grace() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "words")), 0);
    feed.apply(&removed("a", "s1", "sent"), 1_000);
    let c = user_items(&[]);
    feed.settle("s1", &c, 1_000 + GRACE_MS - 1);
    assert_eq!(feed.bubbles("s1").len(), 1);
    feed.settle("s1", &c, 1_000 + GRACE_MS);
    assert!(feed.bubbles("s1").is_empty());
}

#[test]
fn sent_bubbles_come_before_the_rows_still_waiting() {
    let mut feed = OutboxFeed::default();
    feed.apply(&changed(wire("a", "s1", "sending", None, "first")), 0);
    feed.apply(&changed(wire("b", "s1", "queued", None, "second")), 0);
    feed.sent("a", 500);
    let ids: Vec<String> = feed.bubbles("s1").into_iter().map(|b| b.id).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn a_removed_row_that_was_never_seen_leaves_no_sent_bubble() {
    let mut feed = OutboxFeed::default();
    assert!(!feed.apply(&removed("ghost", "s1", "sent"), 0));
    assert!(feed.bubbles("s1").is_empty());
}
