use super::outbox::{bubble, Button, OutboxRow, Status};
use serde_json::json;

fn held(text: &str, boxed: &str) -> OutboxRow {
    OutboxRow::from_wire(&json!({
        "id": "o1", "session": "s1", "tool": "claude", "kind": "prompt", "text": text,
        "state": "box", "why": "not_cleared", "box": boxed, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    }))
    .unwrap()
}

#[test]
fn a_box_holding_this_very_message_says_so() {
    let shown = bubble(&held("the same words", "the same words"));
    assert_eq!(shown.status, Status::BoxHoldsThis);
    assert_eq!(
        shown.buttons,
        [Button::ClearBoxAndSend, Button::Edit, Button::Remove]
    );
}

#[test]
fn spacing_does_not_make_the_same_message_other_text() {
    let shown = bubble(&held("the same\n  words", "the  same words\n"));
    assert_eq!(shown.status, Status::BoxHoldsThis);
}

#[test]
fn a_box_with_other_words_still_names_them() {
    let shown = bubble(&held("the same words", "something else"));
    assert_eq!(shown.status, Status::BoxHasText("something else".into()));
}

#[test]
fn an_unreadable_box_is_not_taken_for_this_message() {
    let shown = bubble(&held("the same words", ""));
    assert_eq!(shown.status, Status::BoxHasText(String::new()));
}
