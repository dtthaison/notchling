use super::exec::{execute, Failure, Markers, NoQueueLog, Outcome, Pty, ScreenCheck};
use super::fake::FakePrompt;
use super::keys::timing::PERMISSION_DRAW_WAIT_MS;
use super::plan::{self, DialogPick};
use std::time::Duration;

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

#[test]
fn a_dismiss_checks_a_dialog_is_open_before_it_sends_its_one_esc() {
    let names: Vec<&str> = plan::dismiss().steps.iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "wait quiet",
            "require any dialog",
            "require at prompt",
            "keys",
            "pause",
            "wait for"
        ]
    );
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes, ["\x1b"]);
    assert!(pty.picked.is_empty(), "no option number typed");
    assert!(!pty.dialog_shown(), "it waited for the dialog to go");
}

struct Working;

impl ScreenCheck for Working {
    fn shows(&self, _question: &str, _lines: &[String]) -> bool {
        false
    }
    fn is_review(&self, _lines: &[String]) -> bool {
        false
    }
    fn at_prompt(&self) -> bool {
        false
    }
}

#[test]
fn a_dismiss_sends_no_esc_once_the_session_is_no_longer_at_a_prompt() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let done = execute(&plan::dismiss(), &mut pty, &Working, &mut NoQueueLog);
    assert_eq!(done, Err(Failure::Working));
    assert!(pty.writes.is_empty(), "no Esc into a working turn");
    assert!(pty.dialog_shown());
}

#[test]
fn a_dismiss_with_no_dialog_open_sends_no_esc() {
    let mut pty = FakePrompt::new(&[
        "⏺ Done.",
        "────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Err(Failure::QuestionGone));
    assert!(pty.writes.is_empty(), "no Esc into an idle prompt");
}

#[test]
fn a_dismiss_also_closes_a_dialog_this_parser_cannot_read() {
    let mut pty = FakePrompt::new(&[]).with_dialog(&[
        "────────────────────────────────────────",
        " Something this parser does not know",
        " Enter to confirm · Esc to cancel",
    ]);
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes, ["\x1b"]);
}

const IDLE: &[&str] = &[
    "⏺ Done.",
    "────────────────────────────────────────",
    "❯",
    "────────────────────────────────────────",
    "  ? for shortcuts",
];

#[test]
fn a_dismiss_with_no_dialog_fails_at_once_without_waiting_for_one() {
    let mut pty = FakePrompt::new(IDLE);
    let before = pty.now_ms();
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Err(Failure::QuestionGone));
    assert!(pty.writes.is_empty());
    assert!(
        pty.now_ms() - before < PERMISSION_DRAW_WAIT_MS,
        "no wait for a dialog to draw: {} ms",
        pty.now_ms() - before
    );
}

#[test]
fn a_dismiss_over_output_that_only_looks_like_a_dialog_sends_no_esc() {
    for screen in [
        &[
            "⏺ The docs say to pick a model like this:",
            "",
            "  Choose a model",
            "  > 1. Opus for hard work",
            "  2. Sonnet for the rest",
        ][..],
        &[
            "⏺ Setup is ready.",
            "  Before you go on",
            "  1. Install the tools",
            "  2. Run the tests",
            "  Press Enter to continue",
        ][..],
        &[
            "⏺ The menu looks like this:",
            "  Pick a model",
            "  ❯ 1. Opus",
            "    2. Sonnet",
            "  Pick one and press Enter.",
        ][..],
        &[
            "⏺ The menu you will see:",
            "  Use the arrows to move.",
            "  ↑/↓ to navigate · Esc to close",
        ][..],
    ] {
        let mut pty = FakePrompt::new(screen);
        let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
        assert_eq!(done, Err(Failure::QuestionGone), "{screen:?}");
        assert!(pty.writes.is_empty(), "no Esc into {screen:?}");
    }
}

struct Stepping {
    screens: Vec<Vec<String>>,
    at: usize,
    clock: u64,
    writes: Vec<String>,
}

impl Pty for Stepping {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        if typed == "\x1b" {
            self.at = (self.at + 1).min(self.screens.len() - 1);
        }
        self.writes.push(typed);
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }
    fn screen(&mut self) -> Vec<String> {
        self.screens[self.at].clone()
    }
    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

#[test]
fn an_esc_that_steps_back_to_another_dialog_has_moved_it() {
    let parent: Vec<String> = [
        "────────────────────────────────────────",
        " Select model",
        " ❯ 1. Opus 5.5",
        "   2. Sonnet 5.5",
        "   3. Haiku 5.5",
        " Enter to confirm · Esc to cancel",
    ]
    .iter()
    .map(|l| l.to_string())
    .collect();
    let mut pty = Stepping {
        screens: vec![SWITCH.iter().map(|l| l.to_string()).collect(), parent],
        at: 0,
        clock: 0,
        writes: Vec::new(),
    };
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes, ["\x1b"], "one Esc, never a second");
}

#[test]
fn an_esc_that_leaves_the_same_dialog_up_is_still_open() {
    let mut pty = Stepping {
        screens: vec![SWITCH.iter().map(|l| l.to_string()).collect()],
        at: 0,
        clock: 0,
        writes: Vec::new(),
    };
    let done = execute(&plan::dismiss(), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(done, Err(Failure::PermissionStillOpen));
    assert_eq!(pty.writes, ["\x1b"]);
}
