//! Plays a [`Plan`] into a terminal. The terminal is anything that is a [`Pty`]: E3a's attach pty
//! through an adapter, or a fake in tests. Screen questions go through [`ScreenCheck`], which E3a's
//! `AskDialog` satisfies; [`Markers`] is the dialog check alone, from the Swift markers.

use super::keys::{self, timing::*, LEFT, SUBMIT};
use super::plan::{Plan, Step, Until};
use super::popall;
use std::path::PathBuf;
use std::time::Duration;

/// The terminal a plan is typed into.
pub trait Pty {
    /// Writes all of `bytes`.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    /// Reads output into the screen until none comes for `quiet`, for at most `timeout`. False when
    /// nothing at all arrived (`ReplySender.waitForQuiet`).
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool;
    /// The screen's lines now, after taking in whatever output is already waiting.
    fn screen(&mut self) -> Vec<String>;
    /// Sleeps (output keeps arriving meanwhile and is read later).
    fn sleep(&mut self, duration: Duration);
    /// A monotonic clock, in milliseconds, for the waits that look again until a deadline.
    fn now_ms(&self) -> u64;
}

/// What the screen shows: an open dialog, which question, the review screen.
pub trait ScreenCheck {
    /// A question or permission dialog is open.
    fn dialog_open(&self, lines: &[String]) -> bool {
        keys::dialog_open(lines)
    }
    /// The open dialog's page is this question.
    fn shows(&self, question: &str, lines: &[String]) -> bool;
    /// The dialog's "Review your answers" screen.
    fn is_review(&self, lines: &[String]) -> bool;
}

/// The dialog check alone: knows no question, sees no review screen. Enough for replies, interrupt
/// and take-back, which only ever refuse a dialog.
#[derive(Clone, Copy, Debug, Default)]
pub struct Markers;

impl ScreenCheck for Markers {
    fn shows(&self, _question: &str, _lines: &[String]) -> bool {
        false
    }
    fn is_review(&self, _lines: &[String]) -> bool {
        false
    }
}

/// Where a take-back reads `popAll` from.
pub trait QueueLog {
    /// Up is about to be pressed: `popAll` lines count from a little before now.
    fn mark(&mut self);
    /// The log has a `popAll` of `text` since the mark.
    fn popped_all(&mut self, text: &str) -> bool;
}

/// A session transcript as the queue log, read by its tail.
pub struct TranscriptLog {
    path: PathBuf,
    since_ms: i64,
}

impl TranscriptLog {
    pub fn new(path: impl Into<PathBuf>) -> TranscriptLog {
        TranscriptLog {
            path: path.into(),
            since_ms: i64::MAX,
        }
    }
}

impl QueueLog for TranscriptLog {
    fn mark(&mut self) {
        self.since_ms = popall::now_ms() - POP_ALL_SLACK_MS as i64;
    }
    fn popped_all(&mut self, text: &str) -> bool {
        popall::popped_all_in_file(&self.path, text, self.since_ms)
    }
}

/// No queue log: nothing is ever seen taken.
pub struct NoQueueLog;

impl QueueLog for NoQueueLog {
    fn mark(&mut self) {}
    fn popped_all(&mut self, _text: &str) -> bool {
        false
    }
}

/// Why typing stopped (`ReplySender.Failure`, with its words).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A write failed.
    Pty(String),
    /// The screen never drew.
    Timeout,
    /// A dialog is open; typing would answer it.
    Dialog,
    /// The question isn't open any more.
    QuestionGone,
    /// The terminal shows another question than the one being answered.
    QuestionMismatch,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Pty(why) => write!(f, "Couldn't type into the session's terminal: {why}"),
            Failure::Timeout => write!(f, "The session didn't open in time."),
            Failure::Dialog => write!(
                f,
                "The session is asking you something. Answer it in the terminal, then send again."
            ),
            Failure::QuestionGone => write!(f, "That question isn't open in the session any more."),
            Failure::QuestionMismatch => write!(
                f,
                "The terminal shows a different question than Chat does, so nothing more was typed. Finish it in the terminal."
            ),
        }
    }
}

impl std::error::Error for Failure {}

/// How a plan ended well.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// A take-back found its `popAll`.
    pub taken: bool,
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Plays `plan`: its steps, then (when they ended well) its detach steps.
pub fn execute(
    plan: &Plan,
    pty: &mut dyn Pty,
    check: &dyn ScreenCheck,
    log: &mut dyn QueueLog,
) -> Result<Outcome, Failure> {
    let mut run = Run {
        pty,
        check,
        log,
        taken: false,
    };
    for step in &plan.steps {
        if !run.step(step)? {
            break;
        }
    }
    for step in &plan.detach {
        run.step(step)?;
    }
    Ok(Outcome { taken: run.taken })
}

struct Run<'a> {
    pty: &'a mut dyn Pty,
    check: &'a dyn ScreenCheck,
    log: &'a mut dyn QueueLog,
    taken: bool,
}

impl Run<'_> {
    fn write(&mut self, keys: &str) -> Result<(), Failure> {
        self.pty
            .write(keys.as_bytes())
            .map_err(|e| Failure::Pty(e.to_string()))
    }

    /// `ReplySender.wait`: read until quiet, look, again until `deadline_ms` from now.
    fn wait_until(&mut self, timeout_ms: u64, done: impl Fn(&Self, &[String]) -> bool) -> bool {
        let deadline = self.pty.now_ms() + timeout_ms;
        loop {
            self.pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
            let lines = self.pty.screen();
            if done(self, &lines) {
                return true;
            }
            if self.pty.now_ms() >= deadline {
                return false;
            }
        }
    }

    /// One step; false ends the steps early (and well).
    fn step(&mut self, step: &Step) -> Result<bool, Failure> {
        match step {
            Step::WaitQuiet {
                quiet_ms,
                timeout_ms,
                required,
            } => {
                let drew = self.pty.wait_quiet(ms(*quiet_ms), ms(*timeout_ms));
                if *required && !drew {
                    return Err(Failure::Timeout);
                }
            }
            Step::RefuseDialog => {
                let lines = self.pty.screen();
                if self.check.dialog_open(&lines) {
                    return Err(Failure::Dialog);
                }
            }
            Step::RequireDialog => {
                let lines = self.pty.screen();
                if !self.check.dialog_open(&lines) {
                    return Err(Failure::QuestionGone);
                }
            }
            Step::Keys(keys) => self.write(keys)?,
            Step::Paste(text) => self.write(&keys::bracketed(text))?,
            Step::Pause(n) => self.pty.sleep(ms(*n)),
            Step::WalkBackTo { question, tries } => {
                let mut pressed = 0;
                loop {
                    let lines = self.pty.screen();
                    if self.check.shows(question, &lines) {
                        break;
                    }
                    if pressed >= *tries {
                        return Err(Failure::QuestionMismatch);
                    }
                    pressed += 1;
                    self.write(LEFT)?;
                    self.pty
                        .wait_quiet(ms(WALK_BACK_QUIET_MS), ms(WALK_BACK_TIMEOUT_MS));
                }
            }
            Step::ExpectQuestion(question) => {
                let lines = self.pty.screen();
                if !self.check.shows(question, &lines) {
                    return Err(Failure::QuestionMismatch);
                }
            }
            Step::WaitFor { until, timeout_ms } => {
                let moved = self.wait_until(*timeout_ms, |run, lines| match until {
                    Until::Shows(q) => run.check.shows(q, lines),
                    Until::ReviewOrGone => {
                        run.check.is_review(lines) || !run.check.dialog_open(lines)
                    }
                });
                if !moved {
                    return Err(Failure::QuestionMismatch);
                }
            }
            Step::SubmitIfReview { timeout_ms } => {
                let lines = self.pty.screen();
                if self.check.is_review(&lines) {
                    self.write(SUBMIT)?;
                    self.wait_until(*timeout_ms, |run, lines| !run.check.dialog_open(lines));
                }
            }
            Step::MarkQueue => self.log.mark(),
            Step::AwaitPopAll { text, timeout_ms } => {
                let deadline = self.pty.now_ms() + timeout_ms;
                while !self.taken && self.pty.now_ms() < deadline {
                    self.pty
                        .wait_quiet(ms(POP_ALL_QUIET_MS), ms(POP_ALL_STEP_MS));
                    self.taken = self.log.popped_all(text);
                }
            }
            Step::EndUnlessTaken => return Ok(self.taken),
        }
        Ok(true)
    }
}
