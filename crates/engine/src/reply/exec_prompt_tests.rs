use super::exec::{execute, execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
use super::exec_tests::{boxed, lines, Ev, Fake, PROMPT};
use super::fake::{RULE, STARTING};
use super::keys::{self, timing::*};
use super::plan;
use std::time::Duration;

fn sleeps(pty: &Fake) -> usize {
    let typed = pty.events.iter().position(|e| matches!(e, Ev::Write(_)));
    pty.events[..typed.unwrap_or(pty.events.len())]
        .iter()
        .filter(|e| matches!(e, Ev::Sleep(_)))
        .count()
}

#[test]
fn a_prompt_whose_cursor_is_never_reported_is_typed_into_after_the_wait() {
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| boxed(keys, screen));
    pty.blind = true;
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(sleeps(&pty) as u64, INPUT_WAIT_MS / INPUT_POLL_MS);
    assert_eq!(pty.writes().len(), 3);
}

#[test]
fn one_look_between_draws_at_the_end_of_the_wait_does_not_refuse_the_reply() {
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| boxed(keys, screen));
    pty.blind = true;
    let mut torn = Torn {
        inner: &mut pty,
        at: ATTACH_QUIET_MS + INPUT_WAIT_MS,
    };
    let out = execute(&plan::reply("hi"), &mut torn, &Markers, &mut NoQueueLog);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes().len(), 3);
}

struct Torn<'a> {
    inner: &'a mut Fake,
    at: u64,
}

impl Pty for Torn<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        match self.inner.now_ms() == self.at {
            true => lines(&[RULE, "❯ "]),
            false => self.inner.screen(),
        }
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.inner.cursor()
    }
    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration)
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}

#[test]
fn a_box_gone_for_good_at_the_end_of_the_wait_gets_no_keys() {
    let mut pty = Fake::new(PROMPT);
    pty.blind = true;
    let mut gone = Gone {
        inner: &mut pty,
        from: ATTACH_QUIET_MS + INPUT_WAIT_MS,
    };
    let out = execute(&plan::reply("hi"), &mut gone, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::NoInput));
    assert!(pty.writes().is_empty());
}

struct Gone<'a> {
    inner: &'a mut Fake,
    from: u64,
}

impl Pty for Gone<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        match self.inner.now_ms() >= self.from {
            true => lines(&["Resume a session"]),
            false => self.inner.screen(),
        }
    }
    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration)
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}

#[test]
fn a_session_that_never_finishes_starting_is_a_timeout_with_nothing_typed() {
    let mut pty = Fake::new(&["⏺ ok", "", RULE, "❯", RULE, STARTING]);
    pty.blind = true;
    let mut trail = Vec::new();
    let out = execute_noting(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    assert_eq!(out, Err(Failure::Timeout));
    assert!(pty.writes().is_empty());
    assert_eq!(sleeps(&pty) as u64, BOOT_WAIT_MS / INPUT_POLL_MS);
    assert_eq!(
        trail,
        [format!("+500ms await input: starting: {}", STARTING.trim())]
    );
}

#[test]
fn the_words_of_a_dialog_in_a_starting_sessions_transcript_refuse_nothing() {
    let mut pty = Fake::new(&[" Do you want to proceed?", "", RULE, "❯", RULE, STARTING]);
    pty.blind = true;
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::Timeout), "not a dialog");
}

#[test]
fn a_box_that_goes_away_after_the_paste_is_not_a_message_the_session_refused() {
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| {
        if keys.starts_with(keys::PASTE_START) {
            *screen = lines(&["Resume a session", "  1. fix-hook"]);
        }
    });
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::NoInput));
    assert_eq!(pty.writes().len(), 2, "no Enter");
}

#[test]
fn text_that_keeps_changing_in_the_box_is_typed_but_not_sent() {
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| boxed(keys, screen));
    let mut flapping = Flapping {
        inner: &mut pty,
        n: 0,
    };
    let out = execute(&plan::reply("hi"), &mut flapping, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::NotSent));
    assert_eq!(pty.writes().len(), 2, "no Enter");
}

struct Flapping<'a> {
    inner: &'a mut Fake,
    n: usize,
}

impl Pty for Flapping<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        let mut lines = self.inner.screen();
        if lines[1].len() > "❯ ".len() {
            self.n += 1;
            lines[1] = format!("{} {}", lines[1], self.n % 7);
        }
        lines
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.inner.cursor().map(|(row, col)| (row, col + 2))
    }
    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration)
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}

#[test]
fn a_cursor_parked_at_the_boxs_start_does_not_hide_the_message_in_it() {
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| boxed(keys, screen));
    let mut parked = Parked(&mut pty);
    let out = execute(
        &plan::reply("hi there"),
        &mut parked,
        &Markers,
        &mut NoQueueLog,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes().len(), 3, "the Enter went out");
}

struct Parked<'a>(&'a mut Fake);

impl Pty for Parked<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.write(bytes)
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.0.wait_quiet(quiet, timeout)
    }
    fn screen(&mut self) -> Vec<String> {
        self.0.screen()
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.0.cursor().map(|(row, _)| (row, 2))
    }
    fn sleep(&mut self, duration: Duration) {
        self.0.sleep(duration)
    }
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
}
