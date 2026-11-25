use notchling_engine::reply::exec::{
    execute_clearing, execute_noting, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use notchling_engine::reply::keys::{self, timing::INPUT_WAIT_MS, ENTER, PASTE_END, PASTE_START};
use notchling_engine::reply::plan;
use notchling_engine::terminal::input_box::{self, Fill};
use notchling_engine::terminal::{Shade, TerminalScreen};
use std::time::Duration;

const ROWS: usize = 50;
const COLS: usize = 200;
const BOX: usize = 46;
const HINT: &str = "Try \"refactor parser.rs\"";
const DIM: &str = "\x1b[2m";
const BRIGHT: &str = "\x1b[22m";

fn at(row: usize, col: usize) -> String {
    format!("\x1b[{};{}H", row + 1, col + 1)
}

fn hint_cells(hint: &str) -> String {
    let mut chars = hint.chars();
    let first = chars.next().unwrap_or(' ');
    format!("\x1b[7m{first}\x1b[27m\x1b[2m{}\x1b[22m", chars.as_str())
}

fn fresh(box_row: &str, native: bool) -> Vec<u8> {
    let mut out = String::from("\x1b[?2004h\x1b[2J");
    out.push_str(&at(1, 0));
    out.push_str(" ▐▛███▛█   Claude Code v2.1.328");
    out.push_str(&at(2, 0));
    out.push_str("▝▜██████▀  Opus 5.5 with medium effort · Claude Ultra");
    out.push_str(&at(3, 0));
    out.push_str(" ▝▝   ▝▝   ~/work");
    out.push_str(&at(BOX - 1, 0));
    out.push_str(&"─".repeat(COLS));
    out.push_str(&at(BOX, 0));
    out.push_str("❯\u{a0}");
    out.push_str(box_row);
    out.push_str(&at(BOX + 1, 0));
    out.push_str(&"─".repeat(COLS));
    out.push_str(&at(BOX, 2));
    out.push_str(if native { "\x1b[?25h" } else { "\x1b[?25l" });
    out.into_bytes()
}

fn empty_box() -> String {
    "\x1b[7m \x1b[27m".to_string()
}

struct Claude {
    screen: TerminalScreen,
    clock: u64,
    held: String,
    hint: bool,
    writes: Vec<String>,
    sent: Vec<String>,
}

impl Claude {
    fn showing(box_row: &str, hint: bool, native: bool) -> Claude {
        let mut screen = TerminalScreen::new(ROWS, COLS);
        screen.feed(&fresh(box_row, native));
        Claude {
            screen,
            clock: 0,
            held: String::new(),
            hint,
            writes: Vec::new(),
            sent: Vec::new(),
        }
    }

    fn fresh_session() -> Claude {
        Claude::showing(&hint_cells(HINT), true, false)
    }

    fn typed_by_a_person(text: &str) -> Claude {
        Claude::showing(&format!("{text}{}", empty_box()), false, false)
    }

    fn redraw(&mut self, row: String) {
        let frame = format!("\x1b[?25l{}\x1b[2K❯\u{a0}{row}{}", at(BOX, 0), at(BOX, 2));
        self.screen.feed(frame.as_bytes());
    }
}

impl Pty for Claude {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        if typed == keys::clear_input() {
            self.held.clear();
            let row = match self.hint {
                true => hint_cells(HINT),
                false => empty_box(),
            };
            self.redraw(row);
        } else if let Some(text) = typed
            .strip_prefix(PASTE_START)
            .and_then(|t| t.strip_suffix(PASTE_END))
        {
            self.held.push_str(text);
            self.redraw(format!("{}{}", self.held, empty_box()));
        } else if typed == ENTER && !self.held.is_empty() {
            self.sent.push(std::mem::take(&mut self.held));
            self.hint = false;
            self.redraw(empty_box());
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
fn a_fresh_sessions_hint_with_no_native_cursor_is_an_empty_box() {
    let mut pty = Claude::fresh_session();
    let lines = pty.screen();
    let (cursor, shades) = (pty.cursor(), pty.shades());
    assert_eq!(cursor, None, "this Claude Code hides the cursor");
    assert_eq!(lines[BOX], format!("❯\u{a0}{HINT}"));
    let drawn = input_box::read_shaded(&lines, cursor, &shades).expect("a box");
    assert!(drawn.empty, "{drawn:?}");
    assert!(input_box::hint_shown(&lines, &shades));
    assert_eq!(drawn.fill("hi"), Fill::Empty);
    assert_eq!(input_box::typed_text_shaded(&lines, cursor, &shades), None);
    let blind = input_box::read(&lines, cursor).expect("a box");
    assert!(
        !blind.empty,
        "without the styles it reads as text: {blind:?}"
    );
}

#[test]
fn a_reply_to_a_fresh_session_goes_in_over_the_hint_without_waiting_for_a_cursor() {
    let mut pty = Claude::fresh_session();
    let (out, trail) = send(&mut pty, "hi");
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"), "one clear, one paste, one Enter");
    assert!(
        pty.clock < INPUT_WAIT_MS,
        "the box with Claude Code's cursor in it is its prompt: {trail:#?}"
    );
}

#[test]
fn the_hint_of_an_older_claude_code_with_the_native_cursor_still_goes_the_same_way() {
    let mut pty = Claude::showing(&format!("{DIM}{HINT}{BRIGHT}"), true, true);
    assert!(pty.cursor().is_some());
    let (out, trail) = send(&mut pty, "hi");
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["hi"]);
}

#[test]
fn a_suggestion_after_a_turn_is_a_hint_too() {
    let mut pty = Claude::showing(&hint_cells("go ahead and draw the next pages"), true, false);
    let (out, trail) = send(&mut pty, "hi");
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["hi"]);
}

#[test]
fn a_message_that_is_the_hint_word_for_word_is_still_sent_once() {
    let mut pty = Claude::fresh_session();
    let (out, trail) = send(&mut pty, HINT);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [HINT]);
    assert_eq!(pty.writes, typed(HINT));
}

#[test]
fn the_same_words_typed_by_a_person_are_refused_and_nothing_is_typed() {
    for text in [HINT, "Try", "x"] {
        let mut pty = Claude::typed_by_a_person(text);
        let (out, trail) = send(&mut pty, "hi");
        assert_eq!(out, Err(Failure::NotCleared), "{text}: {trail:#?}");
        assert!(pty.writes.is_empty(), "{text}: no key over a person's text");
        let lines = pty.screen();
        let typed = input_box::typed_text_shaded(&lines, pty.cursor(), &pty.shades());
        assert_eq!(typed.as_deref(), Some(text));
    }
}

#[test]
fn text_a_person_typed_with_the_cursor_moved_to_its_start_is_refused() {
    let home = "\x1b[7mT\x1b[27mry \"refactor parser.rs\"".to_string();
    let mut pty = Claude::showing(&home, false, false);
    let (out, _) = send(&mut pty, "hi");
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());
}

#[test]
fn a_hint_the_person_starts_overtyping_is_refused() {
    let torn = format!("x{DIM}ry \"refactor parser.rs\"{BRIGHT}");
    let mut pty = Claude::showing(&torn, true, false);
    let (out, _) = send(&mut pty, "hi");
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());
}

#[test]
fn clear_box_and_send_goes_through_a_box_that_shows_the_hint_again_once_cleared() {
    let mut pty = Claude::showing(&format!("my own draft{}", empty_box()), true, false);
    let clearing = Clearing {
        earlier: Vec::new(),
        any: true,
    };
    let mut trail = Vec::new();
    let out = execute_clearing(
        &plan::reply("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
        &clearing,
    );
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["hi"]);
    assert_eq!(pty.writes, typed("hi"));
}
