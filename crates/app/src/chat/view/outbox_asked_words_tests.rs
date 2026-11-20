use super::*;
use crate::chat::outbox::{bubble, OutboxRow};
use notchling_engine::outbox::State;

fn row(why: &str) -> OutboxRow {
    OutboxRow::from_wire(&json!({
        "id": "o1", "session": "preview", "tool": "claude", "kind": "command",
        "text": "/model claude-opus-5-5", "state": "waiting", "why": why, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    }))
    .expect("a row")
}

#[test]
fn a_command_that_asked_something_says_it_needs_your_answer() {
    let shown = bubble(&row("answer"));
    assert_eq!(status_line(&shown.status), "Needs your answer");
}

#[test]
fn a_dialog_that_cannot_be_read_still_says_waiting_for_a_dialog() {
    let shown = bubble(&row("dialog"));
    assert_eq!(status_line(&shown.status), "Waiting for a dialog");
}

#[test]
fn the_terminal_button_has_its_words_and_is_not_a_main_button() {
    assert_eq!(button_label(Button::ShowTerminal), "Show Terminal");
    assert_eq!(button_id(Button::ShowTerminal), "outbox-show-terminal");
    assert!(!is_main(Button::ShowTerminal));
}

#[test]
fn the_dismiss_button_has_its_words_and_is_not_a_main_button() {
    assert_eq!(button_label(Button::Dismiss), "Dismiss");
    assert_eq!(button_id(Button::Dismiss), "outbox-dismiss");
    assert!(!is_main(Button::Dismiss));
}

#[test]
fn a_busy_session_says_what_the_message_waits_for() {
    let said = |why| status_line(&Status::Busy(why));
    assert_eq!(
        said(Busy::Tasks {
            count: 9,
            commands: false
        }),
        "Waiting: 9 background tasks running"
    );
    assert_eq!(
        said(Busy::Tasks {
            count: 1,
            commands: false
        }),
        "Waiting: 1 background task running"
    );
    assert_eq!(
        said(Busy::Tasks {
            count: 3,
            commands: true
        }),
        "Waiting: 3 background commands running"
    );
    assert_eq!(
        said(Busy::Tasks {
            count: 1,
            commands: true
        }),
        "Waiting: 1 background command running"
    );
    assert_eq!(said(Busy::Replying), "Waiting: Claude is replying");
    assert_eq!(
        said(Busy::Unconfirmed),
        "Waiting: an earlier message is unconfirmed"
    );
}

#[test]
fn an_answer_never_seen_says_so_and_offers_the_terminal() {
    let mut unsure = row("answer");
    unsure.state = State::Unsure;
    let mut shown = vec![bubble(&unsure)];
    crate::chat::outbox::offer_terminal(&mut shown);
    assert_eq!(status_line(&shown[0].status), "No answer seen in time");
    assert!(shown[0].buttons.contains(&Button::MarkAsSent));
    assert!(shown[0].buttons.contains(&Button::ShowTerminal));
    assert!(shown[0].buttons.contains(&Button::Dismiss));
}
