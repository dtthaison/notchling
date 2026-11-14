use super::keys::{self, timing::*, ENTER, LEFT, SUBMIT};
use super::plan::{Plan, Step, Until};
use super::popall;
use crate::terminal::input_box::{self, Input};
use crate::terminal::permission_dialog::{self, Decision, PermissionDialog};
use std::path::PathBuf;
use std::time::Duration;

pub trait Pty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool;
    fn screen(&mut self) -> Vec<String>;
    fn cursor(&mut self) -> Option<(usize, usize)> {
        None
    }
    fn sleep(&mut self, duration: Duration);
    fn now_ms(&self) -> u64;
}

pub trait ScreenCheck {
    fn dialog_open(&self, lines: &[String]) -> bool {
        keys::dialog_open(lines)
    }
    fn shows(&self, question: &str, lines: &[String]) -> bool;
    fn is_review(&self, lines: &[String]) -> bool;
}

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

pub trait QueueLog {
    fn mark(&mut self);
    fn popped_all(&mut self, text: &str) -> bool;
}

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

pub struct NoQueueLog;

impl QueueLog for NoQueueLog {
    fn mark(&mut self) {}
    fn popped_all(&mut self, _text: &str) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Pty(String),
    Timeout,
    Dialog,
    NoInput,
    NotCleared,
    NotTaken,
    NotSent,
    QuestionGone,
    QuestionMismatch,
    NotPermission,
    PermissionMismatch,
    NoAlways,
    PermissionStillOpen,
}

impl Failure {
    pub fn is_permission_refusal(&self) -> bool {
        matches!(
            self,
            Failure::NotPermission | Failure::PermissionMismatch | Failure::NoAlways
        )
    }
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
            Failure::NoInput => write!(
                f,
                "The session's terminal isn't at its prompt, so nothing was sent. Open the terminal to see what it shows."
            ),
            Failure::NotCleared => write!(
                f,
                "The session's input box still holds text, so nothing was sent. Clear it in the terminal, then send again."
            ),
            Failure::NotTaken => write!(
                f,
                "The session's terminal didn't take the message, so it wasn't sent. Send it again."
            ),
            Failure::NotSent => write!(
                f,
                "The message is typed in the session's terminal but wasn't sent. Press Return there, or send it again."
            ),
            Failure::QuestionGone => write!(f, "That question isn't open in the session any more."),
            Failure::QuestionMismatch => write!(
                f,
                "The terminal shows a different question than Chat does, so nothing more was typed. Finish it in the terminal."
            ),
            Failure::NotPermission => write!(
                f,
                "The session isn't showing that permission in its terminal any more, so nothing was typed."
            ),
            Failure::PermissionMismatch => write!(
                f,
                "The terminal asks about something else than Chat shows, so nothing was typed. Answer it in the terminal."
            ),
            Failure::NoAlways => write!(
                f,
                "The terminal offers no \"don't ask again\" for this, so nothing was typed. Pick Allow or Deny."
            ),
            Failure::PermissionStillOpen => write!(
                f,
                "The answer was typed, but the permission is still open in the terminal. Check it there."
            ),
        }
    }
}

impl std::error::Error for Failure {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub taken: bool,
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

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
        pressed: None,
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

enum Shown {
    Dialog,
    Other,
    Input(Input),
}

struct Run<'a> {
    pty: &'a mut dyn Pty,
    check: &'a dyn ScreenCheck,
    log: &'a mut dyn QueueLog,
    taken: bool,
    pressed: Option<PermissionDialog>,
}

impl Run<'_> {
    fn write(&mut self, keys: &str) -> Result<(), Failure> {
        self.pty
            .write(keys.as_bytes())
            .map_err(|e| Failure::Pty(e.to_string()))
    }

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

    fn shown(&mut self) -> Shown {
        let lines = self.pty.screen();
        if self.check.dialog_open(&lines) {
            return Shown::Dialog;
        }
        match input_box::read(&lines, self.pty.cursor()) {
            Some(input) => Shown::Input(input),
            None => Shown::Other,
        }
    }

    fn watch(
        &mut self,
        timeout_ms: u64,
        mut done: impl FnMut(&Shown) -> Result<bool, Failure>,
    ) -> Result<Option<Shown>, Failure> {
        let deadline = self.pty.now_ms() + timeout_ms;
        loop {
            let shown = self.shown();
            if done(&shown)? {
                return Ok(None);
            }
            if self.pty.now_ms() >= deadline {
                return Ok(Some(shown));
            }
            self.pty.sleep(ms(INPUT_POLL_MS));
        }
    }

    fn await_input(&mut self, timeout_ms: u64, empty: bool) -> Result<(), Failure> {
        let late = self.watch(timeout_ms, |shown| match shown {
            Shown::Dialog => Err(Failure::Dialog),
            Shown::Other => Ok(false),
            Shown::Input(input) => Ok(input.empty || !empty),
        })?;
        match late {
            None => Ok(()),
            Some(Shown::Input(_)) => Err(Failure::NotCleared),
            Some(_) => Err(Failure::NoInput),
        }
    }

    fn await_paste(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let mut last: Option<Input> = None;
        let late = self.watch(timeout_ms, |shown| match shown {
            Shown::Dialog => Err(Failure::Dialog),
            Shown::Other => Ok(false),
            Shown::Input(input) => {
                let settled = !input.empty && last.as_ref() == Some(input);
                last = Some(input.clone());
                Ok(settled)
            }
        })?;
        match late {
            None => Ok(()),
            Some(_) => Err(Failure::NotTaken),
        }
    }

    fn submit(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        match self.shown() {
            Shown::Dialog => return Err(Failure::Dialog),
            Shown::Other => return Err(Failure::NoInput),
            Shown::Input(input) if input.empty => return Err(Failure::NotTaken),
            Shown::Input(_) => {}
        }
        self.write(ENTER)?;
        let mut let_go = false;
        let late = self.watch(timeout_ms, |shown| {
            let free = match shown {
                Shown::Input(input) => input.empty,
                Shown::Dialog | Shown::Other => true,
            };
            let settled = free && let_go;
            let_go = free;
            Ok(settled)
        })?;
        match late {
            None => Ok(()),
            Some(_) => Err(Failure::NotSent),
        }
    }

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
            Step::AwaitInput { timeout_ms } => self.await_input(*timeout_ms, false)?,
            Step::AwaitEmpty { timeout_ms } => self.await_input(*timeout_ms, true)?,
            Step::Keys(keys) => self.write(keys)?,
            Step::Paste(text) => self.write(&keys::bracketed(text))?,
            Step::AwaitPaste { timeout_ms } => self.await_paste(*timeout_ms)?,
            Step::Submit { timeout_ms } => self.submit(*timeout_ms)?,
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
                    Until::PermissionGone => permission_dialog::read(lines) != run.pressed,
                });
                if !moved {
                    return Err(match until {
                        Until::PermissionGone => Failure::PermissionStillOpen,
                        _ => Failure::QuestionMismatch,
                    });
                }
            }
            Step::PressPermission { decision, expect } => {
                let mut lines = self.pty.screen();
                if permission_dialog::read(&lines).is_none() {
                    self.wait_until(PERMISSION_DRAW_WAIT_MS, |_, lines| {
                        permission_dialog::read(lines).is_some()
                    });
                    lines = self.pty.screen();
                }
                let dialog = permission_dialog::read(&lines).ok_or(Failure::NotPermission)?;
                if !dialog.asks(expect) {
                    return Err(Failure::PermissionMismatch);
                }
                let option = dialog.option_for(*decision).ok_or(match decision {
                    Decision::Always => Failure::NoAlways,
                    _ => Failure::NotPermission,
                })?;
                if !(1..=9).contains(&option.number) {
                    return Err(Failure::NotPermission);
                }
                let digit = option.number.to_string();
                self.pressed = Some(dialog);
                self.write(&digit)?;
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
