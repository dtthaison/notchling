use super::keys::{self, timing::*, ENTER, LEFT, SUBMIT};
use super::plan::{DialogPick, Plan, Step, Until};
use super::popall;
use crate::terminal::input_box::{self, Fill, Input};
use crate::terminal::permission_dialog::{self, Decision, PermissionDialog};
use crate::terminal::screen_dialog::{self, ScreenDialog};
use crate::terminal::Shade;
use std::path::PathBuf;
use std::time::Duration;

pub trait Pty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool;
    fn screen(&mut self) -> Vec<String>;
    fn cursor(&mut self) -> Option<(usize, usize)> {
        None
    }
    fn shades(&mut self) -> Vec<Vec<Shade>> {
        Vec::new()
    }
    fn sleep(&mut self, duration: Duration);
    fn now_ms(&self) -> u64;
    fn wait_attached(&mut self, quiet: Duration, timeout: Duration, _busy: Duration) -> bool {
        self.wait_quiet(quiet, timeout)
    }
    fn stopping(&self) -> bool {
        false
    }
}

impl<P: Pty + ?Sized> Pty for &mut P {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        (**self).write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        (**self).wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        (**self).screen()
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        (**self).cursor()
    }
    fn shades(&mut self) -> Vec<Vec<Shade>> {
        (**self).shades()
    }
    fn sleep(&mut self, duration: Duration) {
        (**self).sleep(duration)
    }
    fn now_ms(&self) -> u64 {
        (**self).now_ms()
    }
    fn wait_attached(&mut self, quiet: Duration, timeout: Duration, busy: Duration) -> bool {
        (**self).wait_attached(quiet, timeout, busy)
    }
    fn stopping(&self) -> bool {
        (**self).stopping()
    }
}

pub fn wait_attached(
    pty: &mut dyn Pty,
    quiet: Duration,
    timeout: Duration,
    busy: Duration,
) -> bool {
    let busy = u64::try_from(busy.as_millis()).unwrap_or(u64::MAX);
    let deadline = pty.now_ms() + u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    let mut drew = false;
    loop {
        let from = pty.now_ms();
        let slice = busy.min(deadline.saturating_sub(from));
        drew |= pty.wait_quiet(quiet, ms(slice));
        let now = pty.now_ms();
        if now.saturating_sub(from) < slice || now >= deadline || pty.stopping() {
            return drew;
        }
        if drew && box_focused(pty) {
            return true;
        }
    }
}

fn box_focused(pty: &mut dyn Pty) -> bool {
    let lines = pty.screen();
    let cursor = pty.cursor();
    let shades = pty.shades();
    !input_box::starting(&lines, cursor)
        && (input_box::focused(&lines, cursor) || input_box::cursor_drawn(&lines, &shades))
}

pub trait ScreenCheck {
    fn dialog_open(&self, lines: &[String]) -> bool {
        keys::dialog_open(lines)
    }
    fn shows(&self, question: &str, lines: &[String]) -> bool;
    fn is_review(&self, lines: &[String]) -> bool;
    fn at_prompt(&self) -> bool {
        true
    }
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
    Mixed,
    QuestionGone,
    QuestionMismatch,
    NotPermission,
    PermissionMismatch,
    NoAlways,
    PermissionStillOpen,
    Working,
    LoginNeeded,
    Wake(String),
}

const WAKE_MARK: &str = "Couldn't wake";
const WAKE_DASH: &str = " — ";
const WAKE_ELSEWHERE: &str = "another terminal";
const WAKE_LOOKBACK: usize = 3;

pub fn wake_failure(lines: &[String]) -> Option<String> {
    let rows: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|row| !row.trim().is_empty())
        .collect();
    let at = (rows.len().saturating_sub(WAKE_LOOKBACK)..rows.len())
        .rev()
        .find(|&i| rows[i].trim_start().starts_with(WAKE_MARK))?;
    let below = &rows[at + 1..];
    if below
        .iter()
        .any(|row| input_box::is_rule(row) || row.trim_start().starts_with('❯'))
    {
        return None;
    }
    let full = |row: &str| row.trim_end().chars().count() >= usize::from(COLS);
    let mut said = rows[at].trim_start().to_string();
    let mut wrapped = full(rows[at]);
    for row in below {
        if !wrapped {
            break;
        }
        said.push_str(row.trim_end());
        wrapped = full(row);
    }
    let said = said.trim();
    Some(match said.split_once(WAKE_DASH) {
        Some((_, reason)) => reason.trim().to_string(),
        None => said.to_string(),
    })
}

pub fn wake_open_elsewhere(reason: &str) -> bool {
    reason.contains(WAKE_ELSEWHERE)
}

impl Failure {
    pub fn or_login(self, signed_out: impl FnOnce() -> bool) -> Failure {
        let hides_login = matches!(self, Failure::QuestionGone | Failure::NotPermission);
        if hides_login && signed_out() {
            Failure::LoginNeeded
        } else {
            self
        }
    }

    pub fn or_login_in(self, transcript: Option<&str>) -> Failure {
        self.or_login(|| transcript.is_some_and(crate::transcript::login::login_needed_in_file))
    }

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
                "The session's input box holds other text, so the message wasn't sent."
            ),
            Failure::NotTaken => write!(
                f,
                "The session's terminal didn't take the message, so it wasn't sent. Send it again."
            ),
            Failure::NotSent => write!(
                f,
                "The message is typed in the session's terminal but wasn't sent. Press Return there, or send it again."
            ),
            Failure::Mixed => write!(
                f,
                "Something else was typed in the session's input box while the message went in, so it wasn't sent. Check the terminal."
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
            Failure::Working => write!(
                f,
                "The session started working, so Esc wasn't sent."
            ),
            Failure::LoginNeeded => write!(
                f,
                "The session's Claude login has expired. Sign in with /login in its terminal, then answer again."
            ),
            Failure::Wake(why) if wake_open_elsewhere(why) => write!(
                f,
                "This conversation is open somewhere else (a terminal or Claude Desktop), so the session can't wake. Close it there, then send again."
            ),
            Failure::Wake(why) => write!(f, "The session couldn't wake: {why}"),
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

pub const TRAIL_MAX: usize = 60;
const TRAIL_ROW: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clearing {
    pub earlier: Vec<String>,
    pub any: bool,
}

pub fn execute(
    plan: &Plan,
    pty: &mut dyn Pty,
    check: &dyn ScreenCheck,
    log: &mut dyn QueueLog,
) -> Result<Outcome, Failure> {
    execute_noting(plan, pty, check, log, &mut Vec::new())
}

pub fn execute_noting(
    plan: &Plan,
    pty: &mut dyn Pty,
    check: &dyn ScreenCheck,
    log: &mut dyn QueueLog,
    trail: &mut Vec<String>,
) -> Result<Outcome, Failure> {
    execute_clearing(plan, pty, check, log, trail, &Clearing::default())
}

pub fn execute_clearing(
    plan: &Plan,
    pty: &mut dyn Pty,
    check: &dyn ScreenCheck,
    log: &mut dyn QueueLog,
    trail: &mut Vec<String>,
    clearing: &Clearing,
) -> Result<Outcome, Failure> {
    let began = pty.now_ms();
    let mut run = Run {
        pty,
        check,
        log,
        clearing,
        taken: false,
        pressed: None,
        pressed_dialog: None,
        closing: None,
        pasted: String::new(),
        intended: plan.steps.iter().find_map(|step| match step {
            Step::Paste(text) => Some(keys::paste_safe(text)),
            _ => None,
        }),
        doing: "",
        began,
        said: String::new(),
        typed: false,
        streak: Streak::default(),
        wake: None,
        trail,
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

#[derive(PartialEq, Eq)]
enum Shown {
    Dialog,
    Starting,
    Other,
    Input(Input, bool),
}

fn ready(shown: &Shown, text: &str) -> bool {
    match shown {
        Shown::Input(input, _) => matches!(input.fill(text), Fill::Whole | Fill::Folded),
        Shown::Dialog | Shown::Starting | Shown::Other => false,
    }
}

fn cut(row: &str) -> String {
    row.trim().chars().take(TRAIL_ROW).collect()
}

fn seen(
    shown: &Shown,
    lines: &[String],
    cursor: Option<(usize, usize)>,
    shades: &[Vec<Shade>],
) -> String {
    let last = || {
        input_box::last_row(lines)
            .map(|r| cut(r))
            .unwrap_or_default()
    };
    let at = match cursor {
        Some((row, col)) => format!("cursor at {row},{col}"),
        None => "cursor hidden".to_string(),
    };
    match shown {
        Shown::Dialog => "a dialog".to_string(),
        Shown::Starting if nothing_drawn(lines) => format!("nothing drawn yet, {at}"),
        Shown::Starting => format!("starting: {}", last()),
        Shown::Other => format!("no input box, {at}, last row: {}", last()),
        Shown::Input(input, focused) => {
            let under = input_box::span(lines)
                .and_then(|(_, bottom)| lines[bottom + 1..].iter().find(|l| !l.trim().is_empty()))
                .map(|r| cut(r))
                .unwrap_or_default();
            format!(
                "input box, {} {}, {}, {at}{}, under it: {under}",
                input.held.len(),
                if input.held.len() == 1 { "row" } else { "rows" },
                match (input.empty, input_box::hint_shown(lines, shades)) {
                    (_, true) => "empty, showing a hint",
                    (true, false) => "empty",
                    (false, false) => "holding text",
                },
                match (*focused, cursor) {
                    (true, Some(_)) => " in the box",
                    (true, None) => ", drawn by the session in the box",
                    (false, _) => "",
                },
            )
        }
    }
}

struct Run<'a> {
    pty: &'a mut dyn Pty,
    check: &'a dyn ScreenCheck,
    log: &'a mut dyn QueueLog,
    clearing: &'a Clearing,
    taken: bool,
    pressed: Option<PermissionDialog>,
    pressed_dialog: Option<ScreenDialog>,
    closing: Option<screen_dialog::Shown>,
    pasted: String,
    intended: Option<String>,
    doing: &'static str,
    began: u64,
    said: String,
    typed: bool,
    streak: Streak,
    wake: Option<String>,
    trail: &'a mut Vec<String>,
}

#[derive(Default)]
struct Streak {
    looked: bool,
    marked: bool,
    stray: bool,
    boxed: Option<u64>,
    focused: Option<u64>,
    fast: bool,
}

impl Streak {
    fn see(&mut self, shown: &Shown, marker: bool, stray: bool, now: u64) {
        let first = !std::mem::replace(&mut self.looked, true);
        self.marked |= marker;
        self.stray = stray;
        let Shown::Input(_, focused) = shown else {
            (self.boxed, self.focused, self.fast) = (None, None, false);
            return;
        };
        self.boxed.get_or_insert(now);
        match (*focused, self.focused) {
            (false, _) => (self.focused, self.fast) = (None, false),
            (true, None) => (self.focused, self.fast) = (Some(now), first),
            (true, Some(_)) => {}
        }
    }

    fn ready(&self, now: u64, late: bool) -> Result<bool, Failure> {
        let stood =
            |from: Option<u64>, ms: u64| from.is_some_and(|at| now.saturating_sub(at) >= ms);
        let ready = match self.focused {
            Some(_) => self.fast || stood(self.focused, START_SETTLE_MS),
            None => {
                stood(self.boxed, INPUT_WAIT_MS) || (late && stood(self.boxed, START_SETTLE_MS))
            }
        };
        match ready && self.stray {
            true => Err(Failure::NoInput),
            false => Ok(ready),
        }
    }
}

fn nothing_drawn(lines: &[String]) -> bool {
    lines.iter().all(|line| line.trim().is_empty())
}

impl Run<'_> {
    fn write(&mut self, keys: &str) -> Result<(), Failure> {
        self.typed = true;
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

    fn note(&mut self, what: &str) {
        let said = format!("{}: {what}", self.doing);
        if said == self.said {
            return;
        }
        if self.trail.len() >= TRAIL_MAX {
            self.trail.remove(TRAIL_MAX / 2);
        }
        let at = self.pty.now_ms().saturating_sub(self.began);
        self.trail.push(format!("+{at}ms {said}"));
        self.said = said;
    }

    fn shown(&mut self) -> Shown {
        let lines = self.pty.screen();
        self.wake = wake_failure(&lines);
        let cursor = self.pty.cursor();
        let shades = self.pty.shades();
        let marker = input_box::starting(&lines, cursor);
        let shown = if marker || nothing_drawn(&lines) {
            Shown::Starting
        } else if self.check.dialog_open(&lines) {
            Shown::Dialog
        } else {
            match input_box::read_shaded(&lines, cursor, &shades)
                .filter(|_| input_box::prompt_row(&lines))
            {
                Some(input) => {
                    let focused = input_box::focused(&lines, cursor)
                        || input_box::cursor_drawn(&lines, &shades);
                    Shown::Input(input, focused)
                }
                None => Shown::Other,
            }
        };
        let stray =
            matches!(shown, Shown::Input(_, false)) && input_box::stray_highlight(&lines, &shades);
        let now = self.pty.now_ms();
        self.streak.see(&shown, marker, stray, now);
        let what = seen(&shown, &lines, cursor, &shades);
        self.note(&what);
        shown
    }

    fn stopped(&mut self) -> Result<(), Failure> {
        if self.typed || !self.pty.stopping() {
            return Ok(());
        }
        self.note("stopping, nothing typed");
        Err(Failure::Timeout)
    }

    fn await_boot(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let deadline = self.pty.now_ms() + timeout_ms;
        loop {
            self.stopped()?;
            let shown = self.shown();
            if let Some(why) = self.wake.take() {
                self.note("the session says it can't wake");
                return Err(Failure::Wake(why));
            }
            let now = self.pty.now_ms();
            let waking = match shown {
                Shown::Starting => true,
                Shown::Other => self.streak.marked,
                Shown::Dialog => false,
                Shown::Input(..) => !self.streak.ready(now, false)?,
            };
            if !waking {
                return Ok(());
            }
            if now >= deadline {
                return Err(Failure::Timeout);
            }
            self.pty.sleep(ms(INPUT_POLL_MS));
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

    fn attach_wait(&mut self, quiet_ms: u64, timeout_ms: u64) -> Result<bool, Failure> {
        let drew = self
            .pty
            .wait_attached(ms(quiet_ms), ms(timeout_ms), ms(ATTACH_BUSY_MS));
        self.stopped()?;
        Ok(drew)
    }

    fn settled(&mut self, mut shown: Shown) -> Shown {
        for _ in 0..SETTLE_LOOKS {
            self.pty.sleep(ms(INPUT_POLL_MS));
            let again = self.shown();
            if again == shown {
                return again;
            }
            shown = again;
        }
        shown
    }

    fn require_free(&mut self, late: bool) -> Result<bool, Failure> {
        let Some(text) = self.intended.clone() else {
            return Ok(true);
        };
        let first = self.shown();
        let shown = match self.free(&first, &text) {
            Err(Failure::NotCleared) => self.settled(first),
            _ => first,
        };
        let now = self.pty.now_ms();
        match shown {
            Shown::Input(..) if self.streak.ready(now, late)? => {
                self.free(&shown, &text).map(|()| true)
            }
            _ => Ok(false),
        }
    }

    fn free(&mut self, shown: &Shown, text: &str) -> Result<(), Failure> {
        match shown {
            Shown::Input(input, _) => match input.fill(text) {
                Fill::Empty | Fill::Whole => Ok(()),
                _ if self.clearing.any => {
                    self.note("clearing the text the box holds, as asked");
                    Ok(())
                }
                _ if input.leftover_of(&self.clearing.earlier) => {
                    self.note("clearing what an earlier try of ours left in the box");
                    Ok(())
                }
                Fill::Part | Fill::Folded | Fill::Foreign => Err(Failure::NotCleared),
            },
            Shown::Dialog => Err(Failure::Dialog),
            Shown::Starting => Err(Failure::Timeout),
            Shown::Other => Err(Failure::NoInput),
        }
    }

    fn closable(&self, lines: &[String]) -> Option<screen_dialog::Shown> {
        screen_dialog::closable(lines, self.check.dialog_open(lines))
    }

    fn closing_still_shown(&self, lines: &[String]) -> bool {
        match (&self.closing, self.closable(lines)) {
            (Some(before), Some(now)) => before.same_as(&now),
            _ => false,
        }
    }

    fn dialog_still_open(&self, lines: &[String]) -> bool {
        match (&self.pressed_dialog, screen_dialog::read(lines)) {
            (Some(pressed), Some(now)) => pressed.same_as(&now),
            _ => false,
        }
    }

    fn press_dialog(&mut self, pick: &DialogPick) -> Result<(), Failure> {
        let mut lines = self.pty.screen();
        if screen_dialog::read(&lines).is_none() {
            self.wait_until(PERMISSION_DRAW_WAIT_MS, |_, lines| {
                screen_dialog::read(lines).is_some()
            });
            lines = self.pty.screen();
        }
        let dialog = screen_dialog::read(&lines).ok_or(Failure::QuestionGone)?;
        if !dialog.offers(pick) || !(1..=9).contains(&pick.number) {
            return Err(Failure::QuestionMismatch);
        }
        self.pressed_dialog = Some(dialog);
        self.write(&pick.number.to_string())
    }

    fn await_prompt(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let start = self.pty.now_ms();
        let mut deadline = start + timeout_ms;
        let mut boxed: Option<u64> = None;
        let mut grace = true;
        loop {
            self.stopped()?;
            let shown = self.shown();
            let now = self.pty.now_ms();
            if shown == Shown::Starting {
                deadline = deadline.max(start + BOOT_WAIT_MS);
            }
            let late = now >= deadline;
            match shown {
                Shown::Dialog => return Err(Failure::Dialog),
                Shown::Input(..) => {
                    if self.streak.ready(now, late)? && self.require_free(late)? {
                        return Ok(());
                    }
                    if now >= deadline + START_SETTLE_MS {
                        return Err(Failure::NoInput);
                    }
                    boxed = Some(now);
                }
                Shown::Starting if late => return Err(Failure::Timeout),
                Shown::Starting => boxed = None,
                Shown::Other if late => {
                    let lately =
                        boxed.is_some_and(|at| now.saturating_sub(at) <= 2 * INPUT_POLL_MS);
                    if !(lately && grace) {
                        return Err(Failure::NoInput);
                    }
                    grace = false;
                    deadline = now + 2 * INPUT_POLL_MS;
                }
                Shown::Other => {}
            }
            self.pty.sleep(ms(INPUT_POLL_MS));
        }
    }

    fn await_empty(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let late = self.watch(timeout_ms, |shown| match shown {
            Shown::Dialog => Err(Failure::Dialog),
            Shown::Starting | Shown::Other => Ok(false),
            Shown::Input(input, _) => Ok(input.empty),
        })?;
        match late {
            None => Ok(()),
            Some(Shown::Input(..)) => {
                self.note(&format!(
                    "the clear keys did not take, the box still holds text after {timeout_ms}ms"
                ));
                Err(Failure::NotCleared)
            }
            Some(Shown::Starting) => Err(Failure::Timeout),
            Some(_) => Err(Failure::NoInput),
        }
    }

    fn await_paste(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let text = self.pasted.clone();
        let mut last: Option<Input> = None;
        let late = self.watch(timeout_ms, |shown| match shown {
            Shown::Dialog => Err(Failure::Dialog),
            Shown::Starting | Shown::Other => Ok(false),
            Shown::Input(input, _) => {
                let settled = last.as_ref() == Some(input);
                last = Some(input.clone());
                match input.fill(&text) {
                    Fill::Foreign if settled => Err(Failure::Mixed),
                    Fill::Whole | Fill::Folded => Ok(settled),
                    Fill::Foreign | Fill::Empty | Fill::Part => Ok(false),
                }
            }
        })?;
        match late {
            None => Ok(()),
            Some(Shown::Input(input, _)) => match input.fill(&text) {
                Fill::Empty => Err(Failure::NotTaken),
                Fill::Foreign => Err(Failure::Mixed),
                Fill::Part | Fill::Whole | Fill::Folded => Err(Failure::NotSent),
            },
            Some(Shown::Starting) => Err(Failure::Timeout),
            Some(_) => Err(Failure::NoInput),
        }
    }

    fn submit(&mut self, timeout_ms: u64) -> Result<(), Failure> {
        let text = self.pasted.clone();
        let first = self.shown();
        let shown = match ready(&first, &text) {
            true => first,
            false => self.settled(first),
        };
        match shown {
            Shown::Dialog => return Err(Failure::Dialog),
            Shown::Starting | Shown::Other => return Err(Failure::NoInput),
            Shown::Input(input, _) => match input.fill(&text) {
                Fill::Empty => return Err(Failure::NotTaken),
                Fill::Whole | Fill::Folded => {}
                Fill::Part | Fill::Foreign => return Err(Failure::Mixed),
            },
        }
        self.write(ENTER)?;
        let mut let_go = false;
        let late = self.watch(timeout_ms, |shown| {
            let free = match shown {
                Shown::Input(input, _) => !input.carries(&text),
                Shown::Dialog | Shown::Starting | Shown::Other => true,
            };
            let settled = free && let_go;
            let_go = free;
            Ok(settled)
        })?;
        match late {
            None => Ok(()),
            Some(Shown::Input(input, _)) if input.carries(&text) => {
                self.note(&format!(
                    "Enter did not take, the box still holds the text after {timeout_ms}ms"
                ));
                Err(Failure::NotSent)
            }
            Some(_) => Err(Failure::Timeout),
        }
    }

    fn step(&mut self, step: &Step) -> Result<bool, Failure> {
        self.stopped()?;
        self.doing = step.name();
        match step {
            Step::WaitQuiet {
                quiet_ms,
                timeout_ms,
                required,
            } => {
                let drew = match required {
                    true => self.attach_wait(*quiet_ms, *timeout_ms)?,
                    false => self.pty.wait_quiet(ms(*quiet_ms), ms(*timeout_ms)),
                };
                if *required && !drew {
                    return Err(Failure::Timeout);
                }
            }
            Step::RefuseDialog => {
                let lines = self.pty.screen();
                let starting = input_box::starting(&lines, self.pty.cursor());
                if !starting && self.check.dialog_open(&lines) {
                    return Err(Failure::Dialog);
                }
            }
            Step::RequireDialog => {
                let lines = self.pty.screen();
                if !self.check.dialog_open(&lines) {
                    return Err(Failure::QuestionGone);
                }
            }
            Step::RequireAnyDialog => {
                let lines = self.pty.screen();
                self.closing = Some(self.closable(&lines).ok_or(Failure::QuestionGone)?);
            }
            Step::RequireAtPrompt => {
                if !self.check.at_prompt() {
                    return Err(Failure::Working);
                }
            }
            Step::AwaitBoot { timeout_ms } => self.await_boot(*timeout_ms)?,
            Step::AwaitInput { timeout_ms } => self.await_prompt(*timeout_ms)?,
            Step::AwaitEmpty { timeout_ms } => self.await_empty(*timeout_ms)?,
            Step::Keys(keys) => self.write(keys)?,
            Step::Paste(text) => {
                self.pasted = keys::paste_safe(text);
                self.write(&keys::bracketed(text))?
            }
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
                    Until::DialogGone => !run.dialog_still_open(lines),
                    Until::DialogClosed => !run.closing_still_shown(lines),
                });
                if !moved {
                    return Err(match until {
                        Until::PermissionGone | Until::DialogGone | Until::DialogClosed => {
                            Failure::PermissionStillOpen
                        }
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
            Step::PressDialog { pick } => self.press_dialog(pick)?,
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
