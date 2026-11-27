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

fn long_script() -> String {
    let body: Vec<String> = (0..6).map(|n| format!("print({n})")).collect();
    format!("!python3 - <<EOF\n{}\nEOF", body.join("\n"))
}

#[test]
fn a_command_of_ours_the_box_folded_is_this_message() {
    let folded = "[Pasted text #1 +6 lines]";
    assert_eq!(
        bubble(&held(&long_script(), folded)).status,
        Status::BoxHoldsThis
    );
    assert_eq!(
        bubble(&held("/review one\ntwo\nthree\nfour", folded)).status,
        Status::BoxHoldsThis
    );
}

#[test]
fn a_command_of_ours_cut_short_for_the_note_is_this_message() {
    let long = format!("!echo {}", "word ".repeat(400));
    let cut: String = long.trim_start_matches('!').chars().take(1000).collect();
    let note = format!("{cut}…");
    assert_eq!(bubble(&held(&long, &note)).status, Status::BoxHoldsThis);
}

#[test]
fn a_fenced_command_is_this_message_by_what_was_typed() {
    let shown = bubble(&held("!```bash\ngit log -1\n```", "git log -1"));
    assert_eq!(shown.status, Status::BoxHoldsThis);
}

#[test]
fn other_text_in_the_box_still_names_it_for_a_command() {
    let folded = "[Pasted text #1 +6 lines]";
    assert_eq!(
        bubble(&held("/clear", folded)).status,
        Status::BoxHasText(folded.into())
    );
    assert_eq!(
        bubble(&held("/review", "/rev")).status,
        Status::BoxHasText("/rev".into())
    );
    assert_eq!(
        bubble(&held("is it right", folded)).status,
        Status::BoxHasText(folded.into())
    );
}
