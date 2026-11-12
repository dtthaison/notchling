//! Answering a permission dialog no hook holds, against a fake terminal: a Write's dialog (which
//! has no "don't ask again"), a dialog that draws after the attach settled, one that never draws,
//! another request in its place, and a path shown shorter than the card's. Made-up screens from
//! Claude Code's known layout, as in `exec_tests` (no recording of the dialog exists yet).

use super::exec::{execute, Failure, Markers, NoQueueLog, Outcome, Pty};
use super::plan::{self, Decision, Expected};
use std::time::Duration;

/// A terminal whose screen is chosen by the clock: time moves only when the executor waits or
/// sleeps, and a digit that the dialog offers closes it.
struct Timed {
    clock: u64,
    writes: Vec<String>,
    /// The screen from each moment on (the last one whose time has come).
    screens: Vec<(u64, Vec<String>)>,
    /// How many options the dialog has: a digit up to it answers.
    options: usize,
    answered: bool,
}

impl Timed {
    fn new(screens: Vec<(u64, Vec<String>)>, options: usize) -> Timed {
        Timed {
            clock: 0,
            writes: Vec::new(),
            screens,
            options,
            answered: false,
        }
    }
}

impl Pty for Timed {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let keys = String::from_utf8_lossy(bytes).into_owned();
        if keys
            .parse::<usize>()
            .is_ok_and(|n| (1..=self.options).contains(&n))
        {
            self.answered = true;
        }
        self.writes.push(keys);
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }
    fn screen(&mut self) -> Vec<String> {
        if self.answered {
            return lines(&["⏺ Write(memory/notes.md)", "> ", "  ? for shortcuts"]);
        }
        let now = self.clock;
        self.screens
            .iter()
            .rev()
            .find(|(at, _)| *at <= now)
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }
    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

fn rule() -> String {
    "─".repeat(120)
}

/// A new file's dialog: its own "allow all edits" in place of "don't ask again".
fn write_dialog(shown_path: &str) -> Vec<String> {
    let rule = rule();
    lines(&[
        "⏺ Write(memory/notes.md)",
        &rule,
        " Create file",
        &format!(" {shown_path}"),
        "   1 # Notes",
        "   2 Something worth remembering.",
        "",
        " Do you want to create notes.md?",
        " ❯ 1. Yes",
        "   2. Yes, allow all edits during this session (shift+tab)",
        "   3. No, and tell Claude what to do differently (esc)",
        "",
        " Esc to cancel",
    ])
}

fn idle() -> Vec<String> {
    lines(&["⏺ Write(memory/notes.md)", "> ", "  ? for shortcuts"])
}

fn notes() -> Expected {
    Expected {
        tool: "Write".into(),
        summary: "/Users/me/work/app/memory/notes.md".into(),
    }
}

fn play(decision: Decision, expect: &Expected, pty: &mut Timed) -> Result<Outcome, Failure> {
    execute(
        &plan::permission(decision, expect),
        pty,
        &Markers,
        &mut NoQueueLog,
    )
}

#[test]
fn always_on_a_files_dialog_is_refused_with_nothing_typed_and_allow_goes_through() {
    // The pair of calls the audit showed: a refusal, then an answer that worked.
    let screen = write_dialog("memory/notes.md");
    let mut pty = Timed::new(vec![(0, screen.clone())], 3);
    assert_eq!(
        play(Decision::Always, &notes(), &mut pty),
        Err(Failure::NoAlways)
    );
    assert!(pty.writes.is_empty());
    let mut pty = Timed::new(vec![(0, screen)], 3);
    assert_eq!(
        play(Decision::Allow, &notes(), &mut pty),
        Ok(Outcome::default())
    );
    assert_eq!(pty.writes, ["1"]);
}

#[test]
fn a_dialog_that_draws_after_the_attach_settled_is_still_answered() {
    // The attach is quiet for 500 ms and the dialog is not on the screen yet (the session was
    // repainting for the attach's size): reading once failed an answer that the next try made.
    let mut pty = Timed::new(
        vec![(0, idle()), (1_400, write_dialog("memory/notes.md"))],
        3,
    );
    assert_eq!(
        play(Decision::Allow, &notes(), &mut pty),
        Ok(Outcome::default())
    );
    assert_eq!(pty.writes, ["1"]);
}

#[test]
fn a_dialog_that_never_draws_ends_the_wait_with_nothing_typed() {
    let mut pty = Timed::new(vec![(0, idle())], 3);
    assert_eq!(
        play(Decision::Allow, &notes(), &mut pty),
        Err(Failure::NotPermission)
    );
    assert!(pty.writes.is_empty());
    // Bounded: the attach's quiet, then a few seconds of looking, then it gives up.
    assert!(pty.clock < 6_000, "waited {} ms", pty.clock);
}

#[test]
fn the_other_request_on_screen_is_a_mismatch_even_after_a_wait() {
    // Two Writes in a row: the terminal asks about the second while the card names the first.
    let other = Expected {
        tool: "Write".into(),
        summary: "/Users/me/work/app/memory/glass-kit.md".into(),
    };
    let mut pty = Timed::new(vec![(0, write_dialog("memory/notes.md"))], 3);
    assert_eq!(
        play(Decision::Allow, &other, &mut pty),
        Err(Failure::PermissionMismatch)
    );
    assert!(pty.writes.is_empty());
}

#[test]
fn a_path_shown_shorter_than_the_cards_is_the_same_request() {
    // The card has the whole path from the job's state; the dialog may show it from the folder,
    // cut in the middle, or from the home folder.
    for shown in [
        "memory/notes.md",
        "…/app/memory/notes.md",
        "~/work/app/memory/notes.md",
        "/Users/me/work/app/memory/notes.md",
    ] {
        let mut pty = Timed::new(vec![(0, write_dialog(shown))], 3);
        assert_eq!(
            play(Decision::Allow, &notes(), &mut pty),
            Ok(Outcome::default()),
            "{shown}"
        );
        assert_eq!(pty.writes, ["1"], "{shown}");
    }
}
