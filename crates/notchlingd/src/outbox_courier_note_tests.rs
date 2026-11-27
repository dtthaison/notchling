use super::*;
use crate::outbox_testkit::rig;
use notchling_engine::reply::fake::{FakePrompt, PROMPT, RULE};

fn noted(text: &str, entered: bool, mut pty: FakePrompt) -> Option<String> {
    let r = rig();
    let row = r.send("job-idle", text);
    r.courier
        .note_of(&row, &Failure::NotSent, entered, &mut pty)
}

fn holding(row: &str) -> FakePrompt {
    FakePrompt::new(&[RULE, row, RULE, "  ? for shortcuts"])
}

#[test]
fn the_text_an_ignored_enter_left_in_the_box_is_the_note() {
    assert_eq!(
        noted("/clear", true, holding("❯ /clear")),
        Some("/clear".into())
    );
    assert_eq!(
        noted("!git push", true, holding("! git push")),
        Some("git push".into())
    );
}

#[test]
fn a_box_seen_empty_after_the_enter_gives_no_note_not_the_command() {
    assert_eq!(noted("/clear", true, FakePrompt::new(&PROMPT)), None);
    assert_eq!(noted("!git push", true, FakePrompt::new(&PROMPT)), None);
}

#[test]
fn only_a_box_that_cannot_be_read_falls_back_to_the_text_we_typed() {
    let gone = FakePrompt::new(&["Resume a session", "  1. fix-hook"]);
    assert_eq!(noted("/clear", true, gone), Some("/clear".into()));
}

#[test]
fn a_paste_that_never_settled_notes_the_failure_never_the_command() {
    let said = Some(Failure::NotSent.to_string());
    for text in ["/clear", "!git push"] {
        assert_eq!(noted(text, false, holding("❯ /cl")), said, "{text}");
        assert_eq!(noted(text, false, FakePrompt::new(&PROMPT)), said, "{text}");
        let gone = FakePrompt::new(&["Resume a session", "  1. fix-hook"]);
        assert_eq!(noted(text, false, gone), said, "{text}");
    }
}

#[test]
fn a_plain_message_whose_enter_was_ignored_keeps_the_failure_as_its_note() {
    let said = Some(Failure::NotSent.to_string());
    assert_eq!(noted("is it right", true, holding("❯ is it right")), said);
    assert_eq!(noted("is it right", true, FakePrompt::new(&PROMPT)), said);
}

#[test]
fn a_picker_between_two_rules_is_an_unreadable_box_not_box_text() {
    let picker =
        FakePrompt::new(&PROMPT).with_dialog(&[RULE, "  Pick a model", RULE, "  Enter to confirm"]);
    assert_eq!(noted("/clear", true, picker), Some("/clear".into()));
}
