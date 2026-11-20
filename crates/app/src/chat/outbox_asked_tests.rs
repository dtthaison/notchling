use super::outbox::{bubble, offer_terminal, Button, OutboxFeed, OutboxRow, Status};
use serde_json::{json, Value};

fn wire(id: &str, state: &str, why: Option<&str>, n: i64) -> Value {
    let mut wire = json!({
        "id": id, "session": "preview", "tool": "claude", "kind": "command",
        "text": "/model claude-opus-5-5", "state": state, "position": n,
        "queuedMs": n, "updatedMs": n, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = json!(why);
    }
    wire
}

fn row(state: &str, why: Option<&str>) -> OutboxRow {
    OutboxRow::from_wire(&wire("o1", state, why, 1)).expect("a row")
}

#[test]
fn a_command_that_asked_something_needs_your_answer_and_is_not_a_failure() {
    let shown = bubble(&row("waiting", Some("answer")));
    assert_eq!(shown.status, Status::NeedsAnswer);
    assert_eq!(shown.buttons, vec![Button::Remove]);
}

#[test]
fn a_dialog_the_daemon_cannot_read_is_still_waiting_for_a_dialog() {
    let shown = bubble(&row("waiting", Some("dialog")));
    assert_eq!(shown.status, Status::WaitingDialog);
}

#[test]
fn a_waiting_dialog_offers_the_terminal_first() {
    let mut shown = vec![bubble(&row("waiting", Some("dialog")))];
    offer_terminal(&mut shown);
    assert_eq!(shown[0].buttons[0], Button::ShowTerminal);
    assert_eq!(shown[0].buttons[1], Button::Dismiss);
    assert_eq!(shown[0].buttons.len(), 4);
    offer_terminal(&mut shown);
    assert_eq!(shown[0].buttons.len(), 4, "offered once");
}

#[test]
fn an_answer_that_waits_offers_the_terminal_too() {
    let mut shown = vec![bubble(&row("waiting", Some("answer")))];
    offer_terminal(&mut shown);
    assert_eq!(
        shown[0].buttons,
        vec![Button::ShowTerminal, Button::Dismiss, Button::Remove]
    );
}

#[test]
fn other_rows_get_no_terminal_button() {
    for (state, why) in [
        ("queued", None),
        ("waiting", Some("turn")),
        ("waiting", Some("retry")),
        ("sending", None),
        ("unsure", Some("not_found")),
    ] {
        let mut shown = vec![bubble(&row(state, why))];
        let before = shown[0].buttons.clone();
        offer_terminal(&mut shown);
        assert_eq!(shown[0].buttons, before, "{state} {why:?}");
        assert!(!shown[0].buttons.contains(&Button::Dismiss));
    }
}

#[test]
fn the_terminal_and_dismiss_buttons_have_no_outbox_request() {
    assert_eq!(Button::ShowTerminal.request("o1", "ab12cd34"), None);
    assert_eq!(Button::Dismiss.request("o1", "ab12cd34"), None);
}

#[test]
fn the_message_behind_an_answer_holds_the_ones_after_it() {
    let mut feed = OutboxFeed::default();
    feed.seed(&[
        wire("o1", "waiting", Some("answer"), 1),
        wire("o2", "queued", None, 2),
    ]);
    let shown = feed.bubbles("preview");
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].status, Status::NeedsAnswer);
    assert_eq!(shown[1].status, Status::WaitsForPrevious);
}
