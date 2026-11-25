mod common;

use common::notchling_fixture;
use notchling_engine::terminal::{dialog_open, keys, TermColor, TerminalScreen};

fn bytes(name: &str) -> Vec<u8> {
    let path = notchling_fixture(&format!("terminal/{name}.bin"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn screen_after(phases: &[&str]) -> TerminalScreen {
    let mut screen = TerminalScreen::new(30, 100);
    for phase in phases {
        screen.feed(&bytes(phase));
    }
    screen
}

fn text(screen: &TerminalScreen) -> String {
    screen.lines().join("\n")
}

#[test]
fn session_draws() {
    let s = screen_after(&["plugin-0-attach"]);
    let t = text(&s);
    assert!(s.alt_screen(), "Claude Code runs full screen");
    assert!(t.contains("Claude Code v2.1.318"), "{t}");
    assert!(t.contains("❯ Reply with just OK."), "{t}");
    assert!(s.bracketed_paste());
    assert!(s.mouse_tracking() && s.sgr_mouse());
    assert_eq!(s.lines().len(), 30);
}

#[test]
fn plugin_menu_draws() {
    let s = screen_after(&["plugin-0-attach", "plugin-1-typed", "plugin-2-enter"]);
    let t = text(&s);
    assert!(t.contains("Discover plugins"), "{t}");
    assert!(t.contains("Installed"), "{t}");
    assert!(t.contains("⌕ Search…"), "{t}");
    assert!(!t.contains("Loading…"), "the spinner line was redrawn over");
}

#[test]
fn escape_leaves_the_menu() {
    let s = screen_after(&[
        "plugin-0-attach",
        "plugin-1-typed",
        "plugin-2-enter",
        "plugin-3-down",
        "plugin-4-esc",
    ]);
    let t = text(&s);
    assert!(!t.contains("Discover plugins"), "{t}");
    assert!(t.contains("❯ /plugin"), "{t}");
}

#[test]
fn wrap_erase_and_color() {
    let mut s = TerminalScreen::new(3, 5);
    s.feed(b"abcdefg");
    assert_eq!(s.lines(), ["abcde", "fg", ""]);
    s.feed(b"\x1b[1;3H\x1b[K\x1b[38;2;1;2;3mX");
    assert_eq!(s.lines()[0], "abX");
    assert_eq!(s.cells()[0][2].style.fg, Some(TermColor::Rgb(1, 2, 3)));
    s.feed(b"\x1b[3;1H\nZ");
    assert_eq!(
        s.lines(),
        ["fg", "", "Z"],
        "a line feed on the last row scrolls"
    );
}

#[test]
fn wide_and_combining_characters() {
    let mut s = TerminalScreen::new(2, 6);
    s.feed("日本e\u{0301}x".as_bytes());
    assert_eq!(s.cells()[0][0].width, 2);
    assert_eq!(s.cells()[0][1].width, 0);
    assert_eq!(s.cells()[0][4].text, "e\u{0301}");
    assert_eq!(s.lines()[0], "日本e\u{0301}x");
}

#[test]
fn answers_cursor_position_but_not_device_attributes() {
    let mut s = TerminalScreen::new(5, 10);
    s.feed(b"\x1b[2;4H\x1b[6n");
    assert_eq!(s.take_replies(), b"\x1b[2;4R");
    s.feed(b"\x1b[c\x1b[>0q\x1b[>c");
    assert!(
        s.take_replies().is_empty(),
        "an answer here cost Claude Code its colors and landed in its input box"
    );
}

#[test]
fn sees_an_open_question() {
    let mut asking = TerminalScreen::new(50, 200);
    asking.feed(&bytes("question-open"));
    assert!(dialog_open(&asking), "{}", text(&asking));
    assert!(
        !dialog_open(&screen_after(&["plugin-0-attach"])),
        "the plain prompt takes replies"
    );
}

#[test]
fn resize_keeps_rows_in_place() {
    let mut s = TerminalScreen::new(4, 6);
    s.feed(b"a\r\nb\r\nc\r\nd");
    s.resize(2, 3);
    assert_eq!(
        s.lines(),
        ["a", "b"],
        "Claude Code repaints only what changed, so rows must not move"
    );
}

#[test]
fn alternate_screen_restores_the_main_one_and_its_cursor() {
    let mut s = TerminalScreen::new(3, 10);
    s.feed(b"main\x1b[2;3H");
    s.feed(b"\x1b[?1049halt");
    assert!(s.alt_screen());
    assert_eq!(
        s.lines(),
        ["", "  alt", ""],
        "the alternate screen starts blank; the cursor stays put (Swift doesn't home it)"
    );
    s.feed(b"\x1b[?1049l");
    assert!(!s.alt_screen());
    assert_eq!(s.lines()[0], "main");
    assert_eq!(s.cursor(), (1, 2), "1049 saves and restores the cursor");
}

#[test]
fn scroll_region_insert_and_delete_lines() {
    let mut s = TerminalScreen::new(4, 4);
    s.feed(b"a\r\nb\r\nc\r\nd");
    s.feed(b"\x1b[2;3r");
    assert_eq!(s.cursor(), (0, 0));
    s.feed(b"\x1b[3;1H\n");
    assert_eq!(
        s.lines(),
        ["a", "c", "", "d"],
        "a line feed at the region's bottom scrolls only the region"
    );
    s.feed(b"\x1b[2;1H\x1b[L");
    assert_eq!(s.lines(), ["a", "", "c", "d"]);
    s.feed(b"\x1b[M");
    assert_eq!(s.lines(), ["a", "c", "", "d"]);
}

#[test]
fn erase_delete_and_insert_characters() {
    let mut s = TerminalScreen::new(1, 6);
    s.feed(b"abcdef\x1b[1;2H\x1b[2P");
    assert_eq!(s.lines()[0], "adef");
    s.feed(b"\x1b[2@");
    assert_eq!(s.lines()[0], "a  def");
    s.feed(b"\x1b[1;5H\x1b[X");
    assert_eq!(s.lines()[0], "a  d f");
    s.feed(b"\x1b[1K");
    assert_eq!(s.lines()[0], "     f");
}

#[test]
fn sgr_styles_and_indexed_colors() {
    let mut s = TerminalScreen::new(1, 8);
    s.feed(b"\x1b[1;3;4;7;31;102mA\x1b[22;23;24;27;39;49mB\x1b[38;5;300mC\x1b[0;95mD\x1b[38mE");
    let row = &s.cells()[0];
    let a = &row[0].style;
    assert!(a.bold && a.italic && a.underline && a.inverse);
    assert_eq!(a.fg, Some(TermColor::Indexed(1)));
    assert_eq!(a.bg, Some(TermColor::Indexed(10)));
    assert_eq!(row[1].style, Default::default());
    assert_eq!(
        row[2].style.fg,
        Some(TermColor::Indexed(255)),
        "clamped, as UInt8(clamping:)"
    );
    assert_eq!(row[3].style.fg, Some(TermColor::Indexed(13)));
    assert_eq!(
        row[4].style.fg, None,
        "38 with nothing after it clears the color"
    );
}

#[test]
fn strings_and_unknown_sequences_are_skipped() {
    let mut s = TerminalScreen::new(1, 20);
    s.feed(b"\x1b]0;title\x07a\x1b]8;;http://x\x1b\\b\x1b(Bc\x1bPq#0\x1b\\d\x1b[?25l");
    assert_eq!(s.lines()[0], "abcd");
    assert!(!s.cursor_visible());
}

#[test]
fn erased_cells_keep_the_background() {
    let mut s = TerminalScreen::new(1, 4);
    s.feed(b"\x1b[44m\x1b[2K");
    assert_eq!(s.cells()[0][0].style.bg, Some(TermColor::Indexed(4)));
    assert_eq!(s.cells()[0][0].style.fg, None);
}

#[test]
fn split_utf8_and_zwj_sequences() {
    let mut s = TerminalScreen::new(1, 10);
    let family = "👨\u{200D}👩".as_bytes();
    for b in family {
        s.feed(&[*b]);
    }
    assert_eq!(
        s.cells()[0][0].text,
        "👨\u{200D}👩",
        "a joiner glues the next character on"
    );
    assert_eq!(s.cells()[0][0].width, 2);
    s.feed(&[0xE6, b'x']);
    assert_eq!(s.lines()[0], "👨\u{200D}👩x");
}

#[test]
fn reset_and_cursor_reports() {
    let mut s = TerminalScreen::new(3, 5);
    s.feed(b"abc\x1b[5n");
    assert_eq!(s.take_replies(), b"\x1b[0n");
    s.feed(b"\x1bc");
    assert_eq!(s.lines(), ["", "", ""]);
    assert_eq!(s.cursor(), (0, 0));
}

#[test]
fn huge_parameters_do_not_overflow() {
    let mut s = TerminalScreen::new(2, 3);
    s.feed(b"\x1b[99999999999999999999999C\x1b[9223372036854775807Bx\x1b[9223372036854775807P");
    assert_eq!(s.cursor(), (1, 2));
}

fn key(code: u16, chars: &str) -> keys::Press {
    keys::Press {
        key_code: code,
        characters: chars.to_string(),
        ..Default::default()
    }
}

#[test]
fn menu_keys() {
    assert_eq!(keys::bytes(&key(126, ""), false).as_deref(), Some("\x1b[A"));
    assert_eq!(keys::bytes(&key(125, ""), true).as_deref(), Some("\x1bOB"));
    assert_eq!(keys::bytes(&key(36, "\r"), false).as_deref(), Some("\r"));
    assert_eq!(keys::bytes(&key(53, ""), false).as_deref(), Some("\x1b"));
    let shift_tab = keys::Press {
        shift: true,
        ..key(48, "\t")
    };
    assert_eq!(keys::bytes(&shift_tab, false).as_deref(), Some("\x1b[Z"));
    assert_eq!(
        keys::bytes(&key(49, " "), false).as_deref(),
        Some(" "),
        "Space toggles a plugin"
    );
}

#[test]
fn control_and_text() {
    let ctrl = |code, chars| keys::Press {
        control: true,
        ..key(code, chars)
    };
    assert_eq!(
        keys::bytes(&ctrl(8, "\u{3}"), false).as_deref(),
        Some("\u{3}")
    );
    assert_eq!(
        keys::bytes(&ctrl(1, "s"), false).as_deref(),
        Some("\u{13}"),
        "ctrl+s filters by marketplace"
    );
    assert_eq!(keys::bytes(&key(0, "ă"), false).as_deref(), Some("ă"));
    assert_eq!(keys::bytes(&key(51, ""), false).as_deref(), Some("\u{7f}"));
}

#[test]
fn mouse_report() {
    assert_eq!(keys::mouse(64, 0, 4, false), "\x1b[<64;1;5M");
    assert_eq!(keys::mouse(0, 9, 2, true), "\x1b[<0;10;3m");
}
