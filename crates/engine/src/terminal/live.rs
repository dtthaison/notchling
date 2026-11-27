use super::pty::{Pty, PtyCommand, PtyError, ReadOutcome};
use super::TerminalScreen;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(100);

pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Terminal {
    pty: Pty,
    screen: TerminalScreen,
    closed: bool,
}

impl Terminal {
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

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.write_timeout(bytes, WRITE_TIMEOUT)
    }

    pub fn write_timeout(&self, bytes: &[u8], timeout: Duration) -> Result<(), PtyError> {
        self.pty.write_all_timeout(bytes, timeout)
    }

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

    pub fn flush_replies(&mut self) -> Result<(), PtyError> {
        let replies = self.screen.take_replies();
        if replies.is_empty() {
            return Ok(());
        }
        self.pty.write_all_timeout(&replies, WRITE_TIMEOUT)
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.screen
            .resize(usize::from(rows.max(1)), usize::from(cols.max(1)));
        self.pty.resize(rows, cols)
    }

    pub fn close(self) -> Option<i32> {
        self.pty.terminate(Duration::from_secs(2))
    }
}
