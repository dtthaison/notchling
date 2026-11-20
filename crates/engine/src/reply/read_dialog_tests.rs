use super::fake::FakePrompt;
use super::read::read_dialog;

const SWITCH: &[&str] = &[
    "────────────────────────────────────────",
    " Switch model?",
    " Your next response will be slower and use more tokens",
    " ❯ 1. Yes, switch to Opus 5.5",
    "   2. No, go back",
    " Enter to confirm · Esc to cancel",
];

#[test]
fn a_dialog_on_the_screen_is_read_without_typing() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let dialog = read_dialog(&mut pty).expect("a read").expect("a dialog");
    assert_eq!(dialog.title, "Switch model?");
    assert_eq!(dialog.options.len(), 2);
    assert_eq!(dialog.options[0].label, "Yes, switch to Opus 5.5");
    assert!(dialog.options[0].selected);
    assert!(pty.writes.is_empty(), "nothing typed");
    assert!(pty.dialog_shown(), "the dialog is left as it was");
}

#[test]
fn a_prompt_with_no_dialog_reads_as_none() {
    let mut pty = FakePrompt::new(&[
        "⏺ Done.",
        "────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    assert_eq!(read_dialog(&mut pty), Ok(None));
    assert!(pty.writes.is_empty(), "nothing typed");
}
