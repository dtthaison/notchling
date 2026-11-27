use super::outbox_rows::{bubble_line, status_line};
use crate::chat::outbox::{bubble, Button, OutboxRow};
use serde_json::json;

fn row(why: &str, error: Option<&str>) -> OutboxRow {
    let mut wire = json!({
        "id": "m1", "session": "ab12cd34", "tool": "claude", "kind": "prompt", "text": "go on",
        "state": "waiting", "why": why, "position": 1, "queuedMs": 10_000, "updatedMs": 10_000,
        "via": "chat",
    });
    if let Some(error) = error {
        wire["error"] = json!(error);
    }
    OutboxRow::from_wire(&wire).expect("a row")
}

#[test]
fn a_session_open_in_another_terminal_says_it_can_not_wake_and_why() {
    let held = bubble(&row(
        "wake",
        Some("Can't open — this session is running in another terminal"),
    ));
    assert_eq!(held.buttons, vec![Button::Edit, Button::Remove]);
    assert_eq!(
        status_line(&held.status),
        "Open somewhere else, so it can't wake. Close it there, trying again"
    );
    assert_eq!(
        bubble_line(&held, 20_000),
        "Open somewhere else, so it can't wake. Close it there, trying again"
    );
}

#[test]
fn any_other_wake_reason_is_shown_as_claude_code_wrote_it() {
    let held = bubble(&row("wake", Some("the job has no conversation left")));
    assert_eq!(
        status_line(&held.status),
        "Couldn't wake the session: the job has no conversation left. Trying again"
    );
}

#[test]
fn a_wake_with_no_reason_still_says_it_could_not_wake() {
    let held = bubble(&row("wake", None));
    assert_eq!(
        status_line(&held.status),
        "Couldn't wake the session. Trying again"
    );
}

#[test]
fn a_plain_retry_keeps_its_own_words() {
    let held = bubble(&row("retry", Some("The session didn't open in time.")));
    assert_eq!(
        status_line(&held.status),
        "Couldn't reach the session, trying again"
    );
}
