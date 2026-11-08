//! A pty with a screen fed from it: what a reply, an answer or a question read is typed through
//! (`ReplySender.attached`, `waitForQuiet` and `wait`), and what Chat's terminal sheet draws.

use super::pty::{Pty, PtyCommand, PtyError, ReadOutcome};
use super::TerminalScreen;
use std::time::{Duration, Instant};

/// How long one read waits before checking the clock again (`waitForQuiet`'s poll of 100 ms).
const POLL: Duration = Duration::from_millis(100);

/// How long [`Terminal::write`] waits for a program that has stopped reading its input.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// A child on a pty and the screen it draws.
#[derive(Debug)]
pub struct Terminal {
    pty: Pty,
    screen: TerminalScreen,
    closed: bool,
}

impl Terminal {
    /// Starts `command` on a pty, with a screen of the same size.
    pub fn spawn(command: &PtyCommand) -> Result<Terminal, PtyError> {
        let pty = Pty::spawn(command)?;
        let screen = TerminalScreen::new(usize::from(command.rows()), usize::from(command.cols()));
        Ok(Terminal {
            pty,
            screen,
            closed: false,
        })
    }

    pub fn screen(&self) -> &TerminalScreen {
        &self.screen
    }

    pub fn pty(&self) -> &Pty {
        &self.pty
    }

    /// True once the child's side has closed: nothing more will arrive.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Writes keys as they are: the caller decides the pauses between them.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.write_timeout(bytes, WRITE_TIMEOUT)
    }

    /// [`Terminal::write`] with its own limit: [`PtyError::Timeout`] when the program hasn't taken
    /// all of it by then.
    pub fn write_timeout(&self, bytes: &[u8], timeout: Duration) -> Result<(), PtyError> {
        self.pty.write_all_timeout(bytes, timeout)
    }

    /// Feeds output into the screen until nothing arrives for `quiet`. False if no output arrived
    /// before `timeout` (`ReplySender.waitForQuiet`). Cursor-position reports the program asks for stay
    /// queued on the screen; [`Terminal::flush_replies`] sends them.
    pub fn wait_for_quiet(&mut self, quiet: Duration, timeout: Duration) -> Result<bool, PtyError> {
        let deadline = Instant::now() + timeout;
        let mut last_output: Option<Instant> = None;
        let mut buf = vec![0u8; 65536];
        while Instant::now() < deadline {
            let wait = POLL.min(deadline.saturating_duration_since(Instant::now()));
            match self.pty.read_timeout(&mut buf, wait)? {
                ReadOutcome::Data(n) => {
                    self.screen.feed(&buf[..n]);
                    last_output = Some(Instant::now());
                }
                ReadOutcome::Closed => {
                    self.closed = true;
                    return Ok(last_output.is_some());
                }
                ReadOutcome::Timeout => {
                    if last_output.is_some_and(|last| last.elapsed() >= quiet) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(last_output.is_some())
    }

    /// Keeps feeding the screen until `done` holds or `timeout` passes (`ReplySender.wait`): rounds of
    /// a 0.3 s quiet within 0.9 s, checking after each. Once the child's side closes, the screen can't
    /// change, so `done` is asked once more and that is the answer.
    pub fn wait_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&TerminalScreen) -> bool,
    ) -> Result<bool, PtyError> {
        let deadline = Instant::now() + timeout;
        loop {
            if !self.closed {
                self.wait_for_quiet(Duration::from_millis(300), Duration::from_millis(900))?;
            }
            if done(&self.screen) {
                return Ok(true);
            }
            if self.closed || Instant::now() >= deadline {
                return Ok(false);
            }
        }
    }

    /// Writes back what the program asked the screen (cursor position, status), as Chat's terminal
    /// sheet does. The hidden reply attach never did, so the reply executor leaves them.
    pub fn flush_replies(&mut self) -> Result<(), PtyError> {
        let replies = self.screen.take_replies();
        if replies.is_empty() {
            return Ok(());
        }
        self.pty.write_all_timeout(&replies, WRITE_TIMEOUT)
    }

    /// Resizes the screen (rows kept in place) and the pty (the child gets SIGWINCH).
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.screen
            .resize(usize::from(rows.max(1)), usize::from(cols.max(1)));
        self.pty.resize(rows, cols)
    }

    /// Ends the child (SIGTERM, then SIGKILL after two seconds) and returns its exit status.
    pub fn close(self) -> Option<i32> {
        self.pty.terminate(Duration::from_secs(2))
    }
}
