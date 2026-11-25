use super::refusal;
use crate::outbox_testkit::rig;
use notchling_engine::outbox::{Row, State};
use notchling_engine::reply::exec::Failure;
use serde_json::{json, Value};

const FENCED: &str = "!```bash\ngit log -1\n```";

fn boxed(text: &str) -> Row {
    let r = rig();
    let mut row = r.send("job-idle", text);
    row.state = State::Box;
    row
}

fn long_script() -> String {
    let body: Vec<String> = (0..6).map(|n| format!("print({n})")).collect();
    format!("!python3 - <<EOF\n{}\nEOF", body.join("\n"))
}

fn said(text: &str, note: &str) -> (String, Option<Value>) {
    let err = refusal(&boxed(text), Some(note.to_string()));
    (err.message, err.data)
}

fn typed_not_sent(note: &str) -> (String, Option<Value>) {
    (
        Failure::NotSent.to_string(),
        Some(json!({"why": "not_sent", "box": note})),
    )
}

fn box_holds_other(note: &str) -> (String, Option<Value>) {
    (
        Failure::NotCleared.to_string(),
        Some(json!({"why": "not_cleared", "box": note})),
    )
}

#[test]
fn a_command_of_ours_the_box_folded_is_typed_not_sent() {
    let note = "[Pasted text #1 +6 lines]";
    assert_eq!(said(&long_script(), note), typed_not_sent(note));
    assert_eq!(
        said("/review one\ntwo\nthree\nfour", note),
        typed_not_sent(note)
    );
}

#[test]
fn a_command_of_ours_cut_short_for_the_note_is_typed_not_sent() {
    let long = format!("!echo {}", "word ".repeat(400));
    let note = crate::acting_ops::shown_box(long.trim().trim_start_matches('!'));
    assert!(note.ends_with('…'));
    assert_eq!(said(&long, &note), typed_not_sent(&note));
}

#[test]
fn a_fenced_command_is_told_by_what_was_typed_not_what_was_sent() {
    assert_eq!(said(FENCED, "git log -1"), typed_not_sent("git log -1"));
}

#[test]
fn text_the_box_holds_that_is_not_ours_is_still_not_cleared() {
    let draft = "my own draft";
    assert_eq!(said("/clear", draft), box_holds_other(draft));
    assert_eq!(said("!git push", draft), box_holds_other(draft));
    let chip = "[Pasted text #1 +6 lines]";
    assert_eq!(said("/clear", chip), box_holds_other(chip));
    assert_eq!(said("/review", "/rev"), box_holds_other("/rev"));
    assert_eq!(said("is it right", chip), box_holds_other(chip));
}

#[test]
fn a_command_with_no_note_says_the_box_holds_other_text() {
    let err = refusal(&boxed("/clear"), None);
    assert_eq!(err.message, Failure::NotCleared.to_string());
    assert_eq!(err.data, Some(json!({"why": "not_cleared"})));
}
