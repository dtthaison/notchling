use super::*;
use crate::chat::outbox::{bubble, OutboxFeed};
use notchling_local_api::{topics, Event};
use serde_json::{json, Value};

const SESSION: &str = "ab12cd34";

fn wire(id: &str, n: i64, state: &str, why: Option<&str>, text: &str) -> Value {
    let mut row = json!({
        "id": id, "session": SESSION, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "position": n, "queuedMs": 10_000 + n, "updatedMs": 10_000 + n,
        "via": "chat",
    });
    if let Some(why) = why {
        row["why"] = json!(why);
    }
    row
}

fn feed(rows: Vec<Value>) -> OutboxFeed {
    let mut feed = OutboxFeed::default();
    feed.seed(&rows);
    feed
}

fn statuses(feed: &OutboxFeed) -> Vec<Status> {
    feed.bubbles(SESSION)
        .into_iter()
        .map(|bubble| bubble.status)
        .collect()
}

fn holding(at: i64) -> Value {
    let mut head = wire("a", 1, "unsure", Some("not_found"), "first");
    head["asideAtMs"] = json!(at);
    head
}

fn removed(id: &str, why: &str) -> Event {
    Event {
        event: topics::OUTBOX_REMOVED.into(),
        data: json!({"id": id, "session": SESSION, "why": why}),
    }
}

#[test]
fn an_unsure_head_counts_down_and_keeps_both_buttons() {
    let row = OutboxRow::from_wire(&holding(500_000)).unwrap();
    assert_eq!(row.aside_at_ms, Some(500_000));
    let shown = bubble(&row);
    assert_eq!(shown.status, Status::MayBeInHolding(500_000));
    assert_eq!(shown.buttons, vec![Button::Retry, Button::MarkAsSent]);
    assert_eq!(
        bubble_line(&shown, 500_000 - 102_000),
        "May already be in the session · Next messages go in 1:42"
    );
    assert_eq!(
        bubble_line(&shown, 500_000 - 1),
        "May already be in the session · Next messages go in 0:01"
    );
    assert_eq!(
        bubble_line(&shown, 600_000),
        "May already be in the session · Next messages go in 0:00"
    );
}

#[test]
fn the_countdown_reads_minutes_and_seconds() {
    assert_eq!(countdown(120_000, 0), "2:00");
    assert_eq!(countdown(61_001, 0), "1:02");
    assert_eq!(countdown(9_000, 0), "0:09");
    assert_eq!(countdown(0, 5_000), "0:00");
}

#[test]
fn a_row_that_stepped_aside_has_no_countdown_and_does_not_hold_the_next() {
    let mut aside = wire("a", 1, "unsure", Some("not_found"), "first");
    aside["asideMs"] = json!(400_000);
    let shown = feed(vec![aside, wire("b", 2, "queued", None, "second")]);
    assert_eq!(statuses(&shown), vec![Status::MayBeIn, Status::Queued]);
    let line = bubble_line(&shown.bubbles(SESSION)[0], 450_000);
    assert_eq!(line, "May already be in the session");
}

#[test]
fn a_second_unsure_in_a_row_has_no_timer() {
    let held = wire("a", 1, "unsure", Some("not_found"), "first");
    let shown = feed(vec![held]);
    assert_eq!(statuses(&shown), vec![Status::MayBeIn]);
}

#[test]
fn rows_behind_a_holding_unsure_head_wait_for_the_message_before() {
    let shown = feed(vec![
        holding(500_000),
        wire("b", 2, "queued", None, "second"),
        wire("c", 3, "queued", None, "third"),
    ]);
    assert_eq!(
        statuses(&shown),
        vec![
            Status::MayBeInHolding(500_000),
            Status::WaitsForPrevious,
            Status::WaitsForPrevious
        ]
    );
    assert_eq!(
        bubble_line(&shown.bubbles(SESSION)[1], 0),
        "Waiting for the message before"
    );
}

#[test]
fn rows_behind_a_sending_head_are_just_queued() {
    let shown = feed(vec![
        wire("a", 1, "sending", None, "first"),
        wire("b", 2, "queued", None, "second"),
    ]);
    assert_eq!(statuses(&shown), vec![Status::Sending, Status::Queued]);
}

#[test]
fn rows_behind_a_box_or_in_queue_head_wait_for_the_message_before() {
    for state in ["box", "stopped", "gone", "in_queue"] {
        let shown = feed(vec![
            wire("a", 1, state, None, "first"),
            wire("b", 2, "queued", None, "second"),
        ]);
        assert_eq!(
            statuses(&shown)[1],
            Status::WaitsForPrevious,
            "behind a {state} head"
        );
    }
}

#[test]
fn a_row_held_by_edit_reads_editing_with_no_buttons_and_the_rows_behind_it_wait() {
    let mut held = wire("a", 1, "queued", None, "first");
    held["heldUntilMs"] = json!(900_000);
    let shown = feed(vec![held, wire("b", 2, "queued", None, "second")]);
    let bubbles = shown.bubbles(SESSION);
    assert_eq!(bubbles[0].status, Status::Editing);
    assert!(bubbles[0].buttons.is_empty());
    assert_eq!(bubbles[1].status, Status::WaitsForPrevious);
    assert_eq!(bubble_line(&bubbles[0], 0), "Editing");
}

#[test]
fn a_waiting_row_held_by_edit_also_reads_editing() {
    let mut held = wire("a", 1, "waiting", Some("turn"), "first");
    held["heldUntilMs"] = json!(900_000);
    assert_eq!(statuses(&feed(vec![held])), vec![Status::Editing]);
}

#[test]
fn a_row_the_daemon_joined_says_how_many_messages_it_carries() {
    let mut joined = wire("a", 1, "sending", None, "one\n\ntwo\n\nthree");
    joined["joined"] = json!(3);
    let shown = feed(vec![joined]);
    let bubbles = shown.bubbles(SESSION);
    assert_eq!(bubbles[0].joined, 3);
    assert_eq!(bubble_line(&bubbles[0], 0), "3 messages · Sending…");
    let alone = feed(vec![wire("b", 1, "sending", None, "one")]);
    assert_eq!(alone.bubbles(SESSION)[0].joined, 1);
    assert_eq!(bubble_line(&alone.bubbles(SESSION)[0], 0), "Sending…");
}

#[test]
fn the_waiting_reasons_name_the_tool() {
    for (tool, name) in [
        ("claude", "Claude Code"),
        ("codex", "Codex"),
        ("antigravity", "Antigravity"),
    ] {
        let mut waiting = wire("a", 1, "waiting", Some("queue"), "x");
        waiting["tool"] = json!(tool);
        let mut in_queue = wire("b", 1, "in_queue", None, "y");
        in_queue["tool"] = json!(tool);
        assert_eq!(
            bubble_line(&feed(vec![waiting]).bubbles(SESSION)[0], 0),
            format!("Waiting for {name} to read the message before")
        );
        assert_eq!(
            bubble_line(&feed(vec![in_queue]).bubbles(SESSION)[0], 0),
            format!("In {name}'s queue")
        );
    }
    let row = OutboxRow::from_wire(&wire("a", 1, "waiting", Some("queue"), "x")).unwrap();
    assert_eq!(bubble(&row).status, Status::WaitsForQueue(Tool::Claude));
}

#[test]
fn the_other_waiting_reasons_read_as_the_design_words() {
    let line = |why: &str| {
        bubble_line(
            &feed(vec![wire("a", 1, "waiting", Some(why), "x")]).bubbles(SESSION)[0],
            0,
        )
    };
    assert_eq!(line("turn"), "Waiting for the turn to end");
    assert_eq!(line("dialog"), "Waiting for a dialog");
    assert_eq!(line("retry"), "Couldn't reach the session, trying again");
}

#[test]
fn mark_as_sent_asks_for_a_removal_that_says_it_was_sent_and_retry_for_a_resend() {
    let marked = Button::MarkAsSent.request("o1", SESSION).unwrap();
    assert_eq!(
        (marked.op, marked.args),
        (
            "outbox.remove",
            json!({"id": "o1", "session": SESSION, "sent": true})
        )
    );
    let retry = Button::Retry.request("o1", SESSION).unwrap();
    assert_eq!(retry.op, "outbox.resend");
}

#[test]
fn edit_holds_a_row_and_cancel_lets_it_go() {
    let held = crate::acting::Request::outbox_hold("o1", SESSION, true);
    assert_eq!(
        (held.op, held.args),
        (
            "outbox.hold",
            json!({"id": "o1", "session": SESSION, "on": true})
        )
    );
    let freed = crate::acting::Request::outbox_hold("o1", SESSION, false);
    assert_eq!(freed.args["on"], json!(false));
}

#[test]
fn a_row_removed_because_it_was_joined_leaves_no_sent_bubble() {
    let mut shown = feed(vec![
        wire("a", 1, "sending", None, "one\n\ntwo"),
        wire("b", 2, "queued", None, "two"),
    ]);
    assert!(shown.apply(&removed("b", "joined"), 20_000));
    let left: Vec<String> = shown
        .bubbles(SESSION)
        .into_iter()
        .map(|bubble| bubble.id)
        .collect();
    assert_eq!(left, vec!["a".to_string()]);
    assert!(shown.covers("b"));
}

#[test]
fn a_row_marked_as_sent_leaves_a_sent_bubble() {
    let mut shown = feed(vec![holding(500_000)]);
    assert!(shown.apply(&removed("a", "sent"), 20_000));
    assert_eq!(statuses(&shown), vec![Status::Sent]);
}

fn texts_of(rows: &[crate::chat::rows::Row]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            crate::chat::rows::Row::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_oldest_row_is_the_first_bubble_whatever_order_the_daemon_tells_it() {
    let mut told_late = OutboxFeed::default();
    for (id, n) in [("c", 3), ("a", 1), ("b", 2)] {
        let event = Event {
            event: topics::OUTBOX_CHANGED.into(),
            data: json!({"row": wire(id, n, "queued", None, id)}),
        };
        assert!(told_late.apply(&event, 0));
    }
    let ids: Vec<String> = told_late
        .bubbles(SESSION)
        .into_iter()
        .map(|bubble| bubble.id)
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
    let seeded = feed(vec![
        wire("c", 3, "queued", None, "c"),
        wire("b", 2, "queued", None, "b"),
        wire("a", 1, "queued", None, "a"),
    ]);
    let ids: Vec<String> = seeded
        .bubbles(SESSION)
        .into_iter()
        .map(|bubble| bubble.id)
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn the_tail_draws_claude_codes_own_queue_first_then_the_mailbox_oldest_first_then_sends_in_flight()
{
    let mailbox = feed(vec![
        holding(500_000),
        wire("b", 2, "queued", None, "second"),
        wire("c", 3, "queued", None, "third"),
    ])
    .bubbles(SESSION);
    let tail = crate::chat::rows::tail_rows(
        &["typed in the terminal".to_owned()],
        None,
        &mailbox,
        &["just sent".to_owned()],
        true,
    );
    assert_eq!(
        texts_of(&tail),
        [
            "typed in the terminal",
            "first",
            "second",
            "third",
            "just sent"
        ]
    );
}

#[test]
fn the_render_states_show_the_head_first_and_nothing_from_the_fixtures_own_queue() {
    for state in [
        "aside_sending",
        "wait_unsure",
        "two_behind_unsure",
        "editing",
    ] {
        let shown = crate::chat::renders::outbox_preview(state).unwrap_or_default();
        assert!(shown.len() >= 2, "{state}");
        assert!(
            matches!(
                shown[0].status,
                Status::MayBeIn | Status::MayBeInHolding(_) | Status::Editing
            ),
            "{state}: the head is the first bubble, not {:?}",
            shown[0].status
        );
    }
    let aside = crate::chat::renders::outbox_preview("aside_sending").unwrap_or_default();
    assert_eq!(aside[0].status, Status::MayBeIn);
    assert_eq!(aside[1].status, Status::Sending);
    let alone = crate::chat::renders::outbox_preview("aside").unwrap_or_default();
    assert_eq!(alone.len(), 1);
}

#[test]
fn the_joined_render_carries_as_many_texts_as_it_says() {
    let shown = crate::chat::renders::outbox_preview("joined").unwrap_or_default();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].joined, 3);
    assert_eq!(shown[0].text.split("\n\n").count(), 3);
    assert_eq!(bubble_line(&shown[0], 0), "3 messages · Sending…");
}
