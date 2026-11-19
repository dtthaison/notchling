use notchling_engine::reply::exec::{
    execute_clearing, wait_attached, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use notchling_engine::reply::keys::timing::INPUT_WAIT_MS;
use notchling_engine::reply::plan;
use notchling_engine::terminal::input_box;
use notchling_engine::terminal::{Shade, TerminalScreen};
use std::time::Duration;

const RULE: &str =
    "\x1b[38;2;136;136;136m────────────────────────────────────────────────────────\x1b[39m";
const MARK: &str = "❯\u{a0}";
const HINT: &str = "\x1b[7mT\x1b[27m\x1b[2mry \"fix parser.rs\"\x1b[22m";
const CURSOR: &str = "\x1b[7m \x1b[27m";

struct Still {
    screen: TerminalScreen,
    clock: u64,
    burn: bool,
    first_write: Option<u64>,
    writes: Vec<String>,
}

impl Still {
    fn showing(rows: &[&str]) -> Still {
        let mut screen = TerminalScreen::new(16, 80);
        let mut frame = String::from("\x1b[2J\x1b[H\x1b[?25l");
        let mut put =
            |row: usize, text: &str| frame.push_str(&format!("\x1b[{};1H{text}", row + 1));
        put(0, "made up earlier output");
        put(1, RULE);
        for (n, row) in rows.iter().enumerate() {
            put(2 + n, row);
        }
        put(2 + rows.len(), RULE);
        put(
            3 + rows.len(),
            "\x1b[38;2;153;153;153mnothing else here\x1b[39m",
        );
        screen.feed(frame.as_bytes());
        Still {
            screen,
            clock: 0,
            burn: false,
            first_write: None,
            writes: Vec::new(),
        }
    }

    fn homed(mut self) -> Still {
        self.screen.feed(b"\x1b[3;3H\x1b[?25h");
        self
    }

    fn busy(mut self) -> Still {
        self.burn = true;
        self
    }
}

impl Pty for Still {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.first_write.get_or_insert(self.clock);
        self.writes
            .push(String::from_utf8_lossy(bytes).into_owned());
        Ok(())
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        let spent = if self.burn { timeout } else { quiet };
        self.clock += spent.as_millis() as u64;
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

fn reply(pty: &mut Still, any: bool) -> Result<Outcome, Failure> {
    let clearing = Clearing {
        earlier: Vec::new(),
        any,
    };
    execute_clearing(
        &plan::reply("hi"),
        pty,
        &Markers,
        &mut NoQueueLog,
        &mut Vec::new(),
        &clearing,
    )
}

fn picker() -> Still {
    Still::showing(&[
        "  Pick a model",
        "\x1b[7m  1. Opus                 \x1b[27m",
        "  2. Sonnet",
    ])
}

fn pointer_picker() -> Still {
    Still::showing(&[
        "❯\u{a0}1. Opus",
        "\x1b[7m  2. Sonnet                \x1b[27m",
        "  3. Haiku",
    ])
}

fn diff() -> Still {
    Still::showing(&[
        "  Edit file parser.rs",
        "\x1b[7m- let old = 1;\x1b[27m",
        "\x1b[7m+ let new = 2;\x1b[27m",
    ])
}

fn permission_like() -> Still {
    Still::showing(&[
        "  Run this command?",
        "\x1b[38;2;177;185;249m❯\u{a0}1. Yes\x1b[39m",
        "  2. Yes, for this folder",
        "\x1b[7m  3. No                    \x1b[27m",
    ])
}

fn nothing_sent(mut pty: Still) {
    for any in [false, true] {
        assert_eq!(reply(&mut pty, any), Err(Failure::NoInput), "any: {any}");
        assert!(pty.writes.is_empty(), "keys went to it: {:?}", pty.writes);
    }
    assert!(pty.now_ms() >= INPUT_WAIT_MS, "it waited for a prompt");
}

#[test]
fn a_picker_between_the_rules_is_not_a_box_to_type_into() {
    let pty = picker();
    let (lines, shades) = (pty_lines(&pty), pty.screen.shades());
    assert!(!input_box::cursor_drawn(&lines, &shades));
    nothing_sent(pty);
}

#[test]
fn a_picker_with_a_pointer_on_its_first_row_is_not_typed_into() {
    let pty = pointer_picker();
    let (lines, shades) = (pty_lines(&pty), pty.screen.shades());
    assert!(!input_box::cursor_drawn(&lines, &shades));
    assert!(input_box::stray_highlight(&lines, &shades));
    nothing_sent(pty);
}

#[test]
fn a_diff_highlight_between_the_rules_is_not_typed_into() {
    let pty = diff();
    let (lines, shades) = (pty_lines(&pty), pty.screen.shades());
    assert!(!input_box::cursor_drawn(&lines, &shades));
    nothing_sent(pty);
}

#[test]
fn a_permission_like_screen_without_the_known_markers_is_not_typed_into() {
    let pty = permission_like();
    let (lines, shades) = (pty_lines(&pty), pty.screen.shades());
    assert!(!input_box::cursor_drawn(&lines, &shades));
    assert!(input_box::stray_highlight(&lines, &shades));
    nothing_sent(pty);
}

fn pty_lines(pty: &Still) -> Vec<String> {
    pty.screen.lines()
}

#[test]
fn the_box_with_the_sessions_cursor_is_still_typed_into_at_once() {
    for row in [format!("{MARK}{HINT}"), format!("{MARK}{CURSOR}")] {
        let mut pty = Still::showing(&[&row]);
        let _ = reply(&mut pty, false);
        assert!(!pty.writes.is_empty(), "{row}");
        let at = pty.first_write.expect("a first key");
        assert!(at < INPUT_WAIT_MS, "{row}: after {at} ms");
    }
}

#[test]
fn the_attach_wait_ends_on_the_cursor_the_session_draws() {
    let mut pty = Still::showing(&[&format!("{MARK}{HINT}")]).busy();
    assert!(wait_attached(
        &mut pty,
        Duration::from_millis(300),
        Duration::from_secs(10),
        Duration::from_secs(2),
    ));
    assert!(pty.now_ms() <= 2_000, "ended at {} ms", pty.now_ms());
}

#[test]
fn the_attach_wait_does_not_end_on_a_highlight_that_is_not_a_cursor() {
    for mut pty in [picker().busy(), pointer_picker().busy(), diff().busy()] {
        assert!(wait_attached(
            &mut pty,
            Duration::from_millis(300),
            Duration::from_secs(10),
            Duration::from_secs(2),
        ));
        assert!(pty.now_ms() >= 10_000, "ended at {} ms", pty.now_ms());
    }
}

#[test]
fn a_draft_with_the_cursor_sent_to_its_start_is_not_cleared_unless_asked() {
    let draft = format!("{MARK}hello there plain");
    let mut pty = Still::showing(&[&draft]).homed();
    assert_eq!(pty.cursor(), Some((2, 2)));
    assert_eq!(reply(&mut pty, false), Err(Failure::NotCleared));
    assert!(
        pty.writes.is_empty(),
        "no key over a draft: {:?}",
        pty.writes
    );
    let mut pty = Still::showing(&[&draft]).homed();
    let _ = reply(&mut pty, true);
    assert!(!pty.writes.is_empty(), "Clear Box and Send clears it");
}
