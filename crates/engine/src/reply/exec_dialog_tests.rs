use super::exec::{execute, Failure, Markers, NoQueueLog, Outcome};
use super::fake::FakePrompt;
use super::plan::{self, DialogPick};

const SWITCH: &[&str] = &[
    "────────────────────────────────────────",
    " Switch model?",
    " Your next response will be slower and use more tokens",
    " This conversation is cached for the current model.",
    " ❯ 1. Yes, switch to Opus 5.5",
    "   2. No, go back",
    " Enter to confirm · Esc to cancel",
];

fn pick(title: &str, number: u64, label: &str) -> DialogPick {
    DialogPick {
        title: title.into(),
        number,
        label: label.into(),
    }
}

fn run(pty: &mut FakePrompt, pick: &DialogPick) -> Result<Outcome, Failure> {
    execute(&plan::dialog(pick), pty, &Markers, &mut NoQueueLog)
}

#[test]
fn the_number_of_the_picked_option_is_typed_and_nothing_else() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let done = run(&mut pty, &pick("Switch model?", 2, "No, go back"));
    assert_eq!(done, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes, ["2"]);
    assert_eq!(pty.picked, ["2"]);
    assert!(!pty.dialog_shown());
}

#[test]
fn a_dialog_with_another_title_is_left_alone() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let done = run(
        &mut pty,
        &pick("Delete everything?", 1, "Yes, switch to Opus 5.5"),
    );
    assert_eq!(done, Err(Failure::QuestionMismatch));
    assert!(pty.writes.is_empty(), "nothing typed");
    assert!(pty.dialog_shown());
}

#[test]
fn an_option_the_dialog_does_not_offer_is_not_typed() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    for wrong in [
        pick("Switch model?", 3, "Maybe"),
        pick("Switch model?", 1, "No, go back"),
        pick("Switch model?", 0, "Yes, switch to Opus 5.5"),
    ] {
        assert_eq!(run(&mut pty, &wrong), Err(Failure::QuestionMismatch));
    }
    assert!(pty.writes.is_empty(), "nothing typed");
}

#[test]
fn nothing_is_typed_when_no_dialog_shows() {
    let mut pty = FakePrompt::new(&[
        "⏺ Done.",
        "────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    let done = run(
        &mut pty,
        &pick("Switch model?", 1, "Yes, switch to Opus 5.5"),
    );
    assert_eq!(done, Err(Failure::QuestionGone));
    assert!(pty.writes.is_empty(), "nothing typed");
}

#[test]
fn the_plan_waits_for_the_dialog_to_close_and_detaches_quietly() {
    let plan = plan::dialog(&pick("Switch model?", 1, "Yes"));
    assert_eq!(plan.writes(), Vec::<String>::new());
    assert!(plan
        .steps
        .iter()
        .any(|s| matches!(s, plan::Step::PressDialog { .. })));
    assert!(plan.steps.iter().any(|s| matches!(
        s,
        plan::Step::WaitFor {
            until: plan::Until::DialogGone,
            ..
        }
    )));
    assert!(!plan.detach.is_empty());
}
