use notchling_engine::reply::exec::{execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
use notchling_engine::reply::keys::{self, ENTER, PASTE_END, PASTE_START};
use notchling_engine::reply::plan;
use notchling_engine::terminal::{Shade, TerminalScreen};
use std::time::Duration;

const ROWS: usize = 50;
const COLS: usize = 200;
const BOX: usize = 44;
const CURSOR: &str = "\x1b[7m \x1b[27m";

fn at(row: usize, col: usize) -> String {
    format!("\x1b[{};{}H", row + 1, col + 1)
}

struct Claude {
    screen: TerminalScreen,
    clock: u64,
    held: String,
    writes: Vec<String>,
    sent: Vec<String>,
}

impl Claude {
    fn showing(first: &str, below: &[&str]) -> Claude {
        let mut screen = TerminalScreen::new(ROWS, COLS);
        let mut out = String::from("\x1b[?2004h\x1b[2J");
        out.push_str(&at(1, 0));
        out.push_str(" ▐▛███▛█   Claude Code v2.1.329");
        out.push_str(&at(BOX - 1, 0));
        out.push_str(&"─".repeat(COLS));
        out.push_str(&at(BOX, 0));
        out.push_str("❯\u{a0}");
        out.push_str(first);
        for (n, row) in below.iter().enumerate() {
            out.push_str(&at(BOX + 1 + n, 0));
            out.push_str(row);
        }
        out.push_str(&at(BOX + 1 + below.len(), 0));
        out.push_str(&"─".repeat(COLS));
        out.push_str(&at(BOX + 2 + below.len(), 0));
        out.push_str("  ⏵⏵ auto mode on (shift+tab to cycle)");
        out.push_str("\x1b[?25l");
        screen.feed(out.as_bytes());
        Claude {
            screen,
            clock: 0,
            held: String::new(),
            writes: Vec::new(),
            sent: Vec::new(),
        }
    }

    fn blank(rows_below: usize) -> Claude {
        Claude::showing(CURSOR, &vec![""; rows_below])
    }

    fn redraw(&mut self, row: String) {
        let frame = format!("\x1b[?25l{}\x1b[2K❯\u{a0}{row}", at(BOX, 0));
        self.screen.feed(frame.as_bytes());
    }
}

impl Pty for Claude {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        if typed == keys::clear_input() {
            self.held.clear();
            self.redraw(CURSOR.to_string());
        } else if let Some(text) = typed
            .strip_prefix(PASTE_START)
            .and_then(|t| t.strip_suffix(PASTE_END))
        {
            self.held.push_str(text);
            self.redraw(format!("{}{CURSOR}", self.held));
        } else if typed == ENTER && !self.held.is_empty() {
            self.sent.push(std::mem::take(&mut self.held));
            self.redraw(CURSOR.to_string());
        }
        self.writes.push(typed);
        Ok(())
    }

    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }

    fn screen(&mut self) -> Vec<String> {
        self.screen.lines()
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.screen.cursor_visible().then(|| self.screen.cursor())
    }

    fn shades(&mut self) -> Vec<Vec<Shade>> {
        self.screen.shades()
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        ENTER.to_string(),
    ]
}

fn send(pty: &mut Claude, text: &str) -> (Result<Outcome, Failure>, Vec<String>) {
    let mut trail = Vec::new();
    let out = execute_noting(
        &plan::reply(text),
        pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    (out, trail)
}

#[test]
fn a_box_with_blank_rows_under_the_prompt_takes_a_reply() {
    for below in [1, 2, 3] {
        let mut pty = Claude::blank(below);
        let (out, trail) = send(&mut pty, "hi");
        assert_eq!(out, Ok(Outcome { taken: false }), "{below}: {trail:#?}");
        assert_eq!(pty.sent, ["hi"], "{below}");
        assert_eq!(
            pty.writes,
            typed("hi"),
            "{below}: one clear, one paste, one Enter"
        );
    }
}

#[test]
fn a_box_with_trailing_space_rows_takes_a_reply() {
    let mut pty = Claude::showing(CURSOR, &["      ", "   "]);
    let (out, trail) = send(&mut pty, "hi");
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["hi"]);
}

#[test]
fn words_on_a_row_under_the_prompt_are_refused_and_nothing_is_typed() {
    for (first, below) in [
        (CURSOR, vec!["  my own words"]),
        (CURSOR, vec!["", "  my own words"]),
        (CURSOR, vec!["   ", "  x"]),
        ("my own words", vec![""]),
    ] {
        let mut pty = Claude::showing(first, &below);
        let (out, trail) = send(&mut pty, "hi");
        assert_eq!(out, Err(Failure::NotCleared), "{below:?}: {trail:#?}");
        assert!(
            pty.writes.is_empty(),
            "{below:?}: no key over a person's text"
        );
        assert!(pty.sent.is_empty());
    }
}
