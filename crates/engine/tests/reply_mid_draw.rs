use notchling_engine::reply::exec::{
    execute_clearing, execute_noting, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use notchling_engine::reply::keys::{self, ENTER, PASTE_END, PASTE_START};
use notchling_engine::reply::plan;
use notchling_engine::terminal::input_box::{self, Fill};
use notchling_engine::terminal::TerminalScreen;
use std::collections::VecDeque;
use std::time::Duration;

const ROWS: usize = 50;
const COLS: usize = 200;
const BOX: usize = 46;
const SUGGESTION: &str = "go ahead and draw the next pages";
const MESSAGE: &str = "1 yes, 2 the shell necklace, 3 they flew, see file:///Users/me/Desktop/book/the%20sea/shell-3.png, 4 chapter 2";
const DRAWN: usize = 10;

fn at(row: usize, col: usize) -> String {
    format!("\x1b[{};{}H", row + 1, col + 1)
}

fn prompt(suggestion: Option<&str>, draft: Option<&str>) -> Vec<u8> {
    let mut out = String::from("\x1b[?2004h\x1b[2J");
    out.push_str(&at(40, 0));
    out.push_str("⏺ The plan is ready. Tell me when the credits are back.");
    out.push_str(&at(42, 0));
    out.push_str("✻ Cooked for 19s · done 9:40 PM");
    out.push_str(&at(BOX - 1, 0));
    out.push_str(&"─".repeat(COLS - 16));
    out.push_str(" quiet-otter ─");
    out.push_str(&at(BOX, 0));
    out.push_str("❯\u{a0}");
    let mut col = 2;
    if let Some(draft) = draft {
        out.push_str(draft);
        col += draft.chars().count();
    } else if let Some(hint) = suggestion {
        out.push_str("\x1b[2m");
        out.push_str(hint);
        out.push_str("\x1b[22m");
    }
    out.push_str(&at(BOX + 1, 0));
    out.push_str(&"─".repeat(COLS));
    out.push_str(&at(BOX + 2, 2));
    out.push_str("book > Opus 5.5 | 300.0k tokens");
    out.push_str(&at(BOX + 3, 2));
    out.push_str("⏵⏵ auto mode on (shift+tab to cycle)");
    out.push_str(&at(BOX, col));
    out.push_str("\x1b[?25h");
    out.into_bytes()
}

fn paste_frame(text: &str, cut: Option<usize>) -> Vec<Vec<u8>> {
    let end = 2 + text.chars().count();
    let split = cut.map_or(text.len(), |n| {
        text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
    });
    let head = format!("\x1b[?25l{}{}", at(BOX, 2), &text[..split]);
    let tail = format!("{}\x1b[K{}\x1b[?25h", &text[split..], at(BOX, end));
    match cut {
        Some(_) => vec![head.into_bytes(), tail.into_bytes()],
        None => vec![format!("{head}{tail}").into_bytes()],
    }
}

fn suggestion_frame(hint: &str) -> Vec<Vec<u8>> {
    vec![
        format!("\x1b[?25l{}\x1b[2m{hint}\x1b[22m", at(BOX, 2)).into_bytes(),
        format!("{}\x1b[?25h", at(BOX, 2)).into_bytes(),
    ]
}

fn repaint_frame(text: &str) -> Vec<Vec<u8>> {
    let split = text
        .char_indices()
        .nth(DRAWN)
        .map_or(text.len(), |(i, _)| i);
    let end = 2 + text.chars().count();
    vec![
        format!("\x1b[?25l{}\x1b[2K❯\u{a0}{}", at(BOX, 0), &text[..split]).into_bytes(),
        format!("{}{}\x1b[?25h", &text[split..], at(BOX, end)).into_bytes(),
    ]
}

fn clear_frame() -> Vec<u8> {
    format!(
        "\x1b[?25l{}\x1b[K\x1b[2m{SUGGESTION}\x1b[22m{}\x1b[?25h",
        at(BOX, 2),
        at(BOX, 2)
    )
    .into_bytes()
}

fn enter_frame() -> Vec<u8> {
    format!("\x1b[?25l{}\x1b[K{}\x1b[?25h", at(BOX, 2), at(BOX, 2)).into_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Torn {
    read: usize,
    writes: usize,
}

struct Frames {
    screen: TerminalScreen,
    in_flight: VecDeque<Vec<u8>>,
    arriving: Vec<(usize, Vec<Vec<u8>>)>,
    cut: Option<usize>,
    reads: usize,
    clock: u64,
    held: String,
    writes: Vec<String>,
    sent: Vec<String>,
    torn: Vec<Torn>,
}

impl Frames {
    fn new(first: Vec<u8>) -> Frames {
        let mut screen = TerminalScreen::new(ROWS, COLS);
        screen.feed(&first);
        Frames {
            screen,
            in_flight: VecDeque::new(),
            arriving: Vec::new(),
            cut: None,
            reads: 0,
            clock: 0,
            held: String::new(),
            writes: Vec::new(),
            sent: Vec::new(),
            torn: Vec::new(),
        }
    }

    fn pasted_in(mut self, cut: usize) -> Frames {
        self.cut = Some(cut);
        self
    }

    fn arriving_at(mut self, read: usize, chunks: Vec<Vec<u8>>) -> Frames {
        self.arriving.push((read, chunks));
        self
    }

    fn drain(&mut self) {
        while let Some(chunk) = self.in_flight.pop_front() {
            self.screen.feed(&chunk);
        }
    }
}

impl Pty for Frames {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        if let Some(text) = typed
            .strip_prefix(PASTE_START)
            .and_then(|t| t.strip_suffix(PASTE_END))
        {
            self.held = text.to_string();
            self.in_flight.extend(paste_frame(text, self.cut));
        } else if typed == ENTER {
            self.sent.push(std::mem::take(&mut self.held));
            self.in_flight.push_back(enter_frame());
        } else if typed == keys::clear_input() {
            self.held.clear();
            self.in_flight.push_back(clear_frame());
        }
        self.writes.push(typed);
        Ok(())
    }

    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        self.drain();
        true
    }

    fn screen(&mut self) -> Vec<String> {
        self.reads += 1;
        let now = self.reads;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.arriving)
            .into_iter()
            .partition(|(read, _)| *read <= now);
        self.arriving = later;
        for (_, chunks) in due {
            self.in_flight.extend(chunks);
        }
        if let Some(chunk) = self.in_flight.pop_front() {
            self.screen.feed(&chunk);
        }
        if !self.in_flight.is_empty() {
            self.torn.push(Torn {
                read: now,
                writes: self.writes.len(),
            });
        }
        self.screen.lines()
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.screen.cursor_visible().then(|| self.screen.cursor())
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
        self.drain();
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn send(pty: &mut Frames, text: &str) -> (Result<Outcome, Failure>, Vec<String>) {
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

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        ENTER.to_string(),
    ]
}

#[test]
fn a_frame_caught_mid_draw_shows_the_suggestion_as_text_with_no_cursor() {
    let mut pty = Frames::new(prompt(Some(SUGGESTION), None));
    let lines = pty.screen();
    let cursor = pty.cursor();
    let at_rest = input_box::read(&lines, cursor).unwrap();
    assert!(
        at_rest.empty,
        "the cursor at the box's start marks the suggestion"
    );
    assert_eq!(at_rest.fill(MESSAGE), Fill::Empty);
    for cut in [0, DRAWN] {
        let mut pty = Frames::new(prompt(Some(SUGGESTION), None)).pasted_in(cut);
        pty.write(keys::bracketed(MESSAGE).as_bytes()).unwrap();
        let lines = pty.screen();
        let cursor = pty.cursor();
        assert_eq!(cursor, None, "{cut}: the frame hid the cursor first");
        let caught = input_box::read(&lines, cursor).unwrap();
        assert_eq!(caught.fill(MESSAGE), Fill::Foreign, "{cut}: {caught:?}");
        pty.sleep(Duration::from_millis(100));
        let lines = pty.screen();
        let cursor = pty.cursor();
        let drawn = input_box::read(&lines, cursor).unwrap();
        assert_eq!(drawn.fill(MESSAGE), Fill::Whole, "{cut}: {drawn:?}");
        assert!(cursor.is_some());
    }
}

#[test]
fn a_paste_caught_mid_draw_over_the_suggestion_is_sent_once_it_is_drawn() {
    for cut in [0, DRAWN] {
        let mut pty = Frames::new(prompt(Some(SUGGESTION), None)).pasted_in(cut);
        let (out, trail) = send(&mut pty, MESSAGE);
        assert_eq!(out, Ok(Outcome { taken: false }), "{cut}: {trail:#?}");
        assert_eq!(pty.sent, [MESSAGE], "{cut}");
        assert_eq!(pty.writes, typed(MESSAGE), "{cut}: one paste, one Enter");
        assert!(
            pty.torn.iter().any(|t| t.writes == 2),
            "{cut}: a look fell between the paste's draws: {:?}",
            pty.torn
        );
    }
}

#[test]
fn a_suggestion_drawn_while_the_box_is_checked_is_not_taken_for_text_in_it() {
    let mut pty = Frames::new(prompt(None, None)).arriving_at(3, suggestion_frame(SUGGESTION));
    let (out, trail) = send(&mut pty, MESSAGE);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [MESSAGE]);
    assert_eq!(pty.writes, typed(MESSAGE));
    assert_eq!(
        pty.torn.first().map(|t| t.writes),
        Some(0),
        "the look fell inside the suggestion's draw, before any key: {:?}",
        pty.torn
    );
}

#[test]
fn a_repaint_caught_just_before_enter_does_not_stop_the_message() {
    let mut pty =
        Frames::new(prompt(Some(SUGGESTION), None)).arriving_at(8, repaint_frame(MESSAGE));
    let (out, trail) = send(&mut pty, MESSAGE);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [MESSAGE]);
    assert_eq!(pty.writes, typed(MESSAGE));
    assert_eq!(
        pty.torn.first().map(|t| t.writes),
        Some(2),
        "the look fell inside the repaint, after the paste and before Enter: {:?}",
        pty.torn
    );
}

#[test]
fn text_of_a_persons_own_is_still_refused_once_the_screen_has_settled() {
    let mut pty = Frames::new(prompt(Some(SUGGESTION), Some("my own draft")));
    let (out, _) = send(&mut pty, MESSAGE);
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty(), "no key over a person's draft");

    let beside = format!(
        "\x1b[?25l{} and mine\x1b[?25h",
        at(BOX, 2 + MESSAGE.chars().count())
    );
    let mut pty =
        Frames::new(prompt(Some(SUGGESTION), None)).arriving_at(6, vec![beside.into_bytes()]);
    let (out, _) = send(&mut pty, MESSAGE);
    assert_eq!(out, Err(Failure::Mixed));
    assert_eq!(pty.writes, typed(MESSAGE)[..2], "no Enter");
    assert!(pty.sent.is_empty());
}

#[test]
fn clear_box_and_send_empties_the_box_and_sends_through_a_paste_caught_mid_draw() {
    let ours = || vec![MESSAGE.to_string()];
    for (left, clearing) in [
        (MESSAGE, Clearing::default()),
        (
            MESSAGE,
            Clearing {
                earlier: ours(),
                any: false,
            },
        ),
        (
            MESSAGE,
            Clearing {
                earlier: Vec::new(),
                any: true,
            },
        ),
        (
            "my own draft",
            Clearing {
                earlier: Vec::new(),
                any: true,
            },
        ),
    ] {
        let mut pty = Frames::new(prompt(Some(SUGGESTION), Some(left))).pasted_in(DRAWN);
        let mut trail = Vec::new();
        let out = execute_clearing(
            &plan::reply(MESSAGE),
            &mut pty,
            &Markers,
            &mut NoQueueLog,
            &mut trail,
            &clearing,
        );
        assert_eq!(
            out,
            Ok(Outcome { taken: false }),
            "{left} {clearing:?}: {trail:#?}"
        );
        assert_eq!(pty.sent, [MESSAGE], "{left}");
        assert_eq!(
            pty.writes,
            typed(MESSAGE),
            "{left}: one clear, one paste, one Enter"
        );
    }
}
