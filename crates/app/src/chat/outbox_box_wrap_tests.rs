use super::outbox::{bubble, OutboxRow, Status};
use serde_json::json;

fn held(why: &str, text: &str, boxed: &str) -> OutboxRow {
    OutboxRow::from_wire(&json!({
        "id": "o1", "session": "s1", "tool": "claude", "kind": "prompt", "text": text,
        "state": "box", "why": why, "box": boxed, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    }))
    .unwrap()
}

#[test]
fn a_message_the_box_wrapped_inside_a_long_link_is_still_this_message() {
    let link = format!(
        "file:///Users/me/Desktop/book/{}/shell-3.png",
        "a%20b".repeat(40)
    );
    let text = format!("see {link}, page 2");
    let cut = link.char_indices().nth(150).map(|(i, _)| i).unwrap();
    let boxed = format!("see {}\n{}, page 2", &link[..cut], &link[cut..]);
    for why in ["mixed", "not_cleared"] {
        let shown = bubble(&held(why, &text, &boxed));
        assert_eq!(shown.status, Status::BoxHoldsThis, "{why}");
    }
}

#[test]
fn a_message_whose_picture_became_a_chip_is_still_this_message() {
    let text = "is this the pendant?\n\n/Users/me/Desktop/book/pendant-3.png";
    let shown = bubble(&held("mixed", text, "[Image #4]is this the pendant?"));
    assert_eq!(shown.status, Status::BoxHoldsThis);
}

#[test]
fn more_words_or_a_chip_of_the_persons_are_still_other_text() {
    let text = "is this the pendant?";
    for boxed in [
        "is this the pendant? and mine",
        "[Image #4]is this the pendant?",
        "is this the",
    ] {
        let shown = bubble(&held("mixed", text, boxed));
        assert_eq!(shown.status, Status::BoxHasText(boxed.into()), "{boxed}");
    }
}

#[test]
fn a_message_with_no_letters_is_this_message_only_when_the_box_shows_it() {
    for text in ["👍", "???"] {
        let shown = bubble(&held("mixed", text, "my own draft"));
        assert_eq!(
            shown.status,
            Status::BoxHasText("my own draft".into()),
            "{text}"
        );
        let shown = bubble(&held("mixed", text, text));
        assert_eq!(shown.status, Status::BoxHoldsThis, "{text}");
    }
}
