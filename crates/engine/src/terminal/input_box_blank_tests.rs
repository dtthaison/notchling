use super::input_box::{prompt_row, read, read_shaded, typed_text_shaded, Fill, Input};
use super::screen::{Shade, TerminalScreen};

const RULE: &str =
    "\x1b[38;2;136;136;136m────────────────────────────────────────────────────────\x1b[39m";
const MARK: &str = "❯\u{a0}";
const CURSOR: &str = "\x1b[7m \x1b[27m";
const HINT: &str = "\x1b[7mT\x1b[27m\x1b[2mry \"fix parser.rs\"\x1b[22m";

struct Drawn {
    lines: Vec<String>,
    shades: Vec<Vec<Shade>>,
    cursor: Option<(usize, usize)>,
}

impl Drawn {
    fn input(&self) -> Input {
        read_shaded(&self.lines, self.cursor, &self.shades).expect("a box")
    }

    fn text(&self) -> Option<String> {
        typed_text_shaded(&self.lines, self.cursor, &self.shades)
    }
}

fn drawn(rows: &[&str], cursor: Option<(usize, usize)>) -> Drawn {
    let mut screen = TerminalScreen::new(14, 80);
    let mut frame = String::from("\x1b[2J\x1b[H");
    let mut put = |row: usize, text: &str| frame.push_str(&format!("\x1b[{};1H{text}", row + 1));
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
    frame.push_str(&match cursor {
        Some((row, col)) => format!("\x1b[{};{}H\x1b[?25h", row + 1, col + 1),
        None => "\x1b[?25l".to_string(),
    });
    screen.feed(frame.as_bytes());
    Drawn {
        lines: screen.lines(),
        shades: screen.shades(),
        cursor: screen.cursor_visible().then(|| screen.cursor()),
    }
}

fn blank_boxes() -> Vec<Vec<String>> {
    let cursor_row = format!("{MARK}{CURSOR}");
    let hint_row = format!("{MARK}{HINT}");
    let rows = |first: &str, rest: &[&str]| {
        let mut all = vec![first.to_string()];
        all.extend(rest.iter().map(|row| row.to_string()));
        all
    };
    vec![
        rows(MARK, &[""]),
        rows(MARK, &["", ""]),
        rows(MARK, &["   "]),
        rows(MARK, &["  ", "    ", ""]),
        rows(MARK, &["\u{a0}\u{a0}"]),
        rows(&cursor_row, &[""]),
        rows(&cursor_row, &["   ", "   "]),
        rows(&hint_row, &[""]),
        rows(&hint_row, &["   "]),
    ]
}

#[test]
fn a_box_of_the_mark_and_blank_rows_is_an_empty_box() {
    for rows in blank_boxes() {
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        for cursor in [None, Some((2, 2))] {
            let screen = drawn(&rows, cursor);
            let input = screen.input();
            assert!(input.empty, "{rows:?} {cursor:?}: {input:?}");
            assert_eq!(input.fill("hi"), Fill::Empty, "{rows:?} {cursor:?}");
            assert!(!input.holds("hi"), "{rows:?} {cursor:?}");
            assert_eq!(screen.text(), None, "{rows:?} {cursor:?}");
            assert!(prompt_row(&screen.lines), "{rows:?}");
        }
    }
}

#[test]
fn blank_rows_are_empty_without_any_styles_too() {
    for rows in [
        vec![MARK, ""],
        vec![MARK, "", ""],
        vec![MARK, "   "],
        vec![MARK, "  ", ""],
    ] {
        let screen = drawn(&rows, None);
        let input = read(&screen.lines, None).expect("a box");
        assert!(input.empty, "{rows:?}: {input:?}");
        assert_eq!(input.fill("hi"), Fill::Empty, "{rows:?}");
    }
}

#[test]
fn text_on_any_row_of_the_box_is_still_text() {
    let cursor_row = format!("{MARK}{CURSOR}");
    for rows in [
        vec![format!("{MARK}hello"), String::new()],
        vec![MARK.to_string(), "  my words".to_string()],
        vec![MARK.to_string(), String::new(), "  my words".to_string()],
        vec![cursor_row.clone(), "  my words".to_string()],
        vec![cursor_row.clone(), "   ".to_string(), "  x".to_string()],
        vec![format!("{MARK}hello"), "  more".to_string()],
    ] {
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        for cursor in [None, Some((2, 2))] {
            let screen = drawn(&rows, cursor);
            let input = screen.input();
            assert!(!input.empty, "{rows:?} {cursor:?}: {input:?}");
            assert_eq!(input.fill("hi"), Fill::Foreign, "{rows:?} {cursor:?}");
            assert!(input.holds("hi"), "{rows:?} {cursor:?}");
            assert!(screen.text().is_some(), "{rows:?} {cursor:?}");
        }
    }
}

#[test]
fn a_symbol_on_a_row_under_the_mark_is_not_guessed_away() {
    for symbol in ["⧉", "👍", "?", "..."] {
        let below = drawn(&[MARK, &format!("  {symbol}")], None);
        assert!(!below.input().empty, "{symbol}");
        assert_eq!(below.input().fill("hi"), Fill::Foreign, "{symbol}");
        let beside = drawn(&[&format!("{MARK}{symbol}"), ""], None);
        assert!(!beside.input().empty, "{symbol}");
        assert_eq!(beside.input().fill("hi"), Fill::Foreign, "{symbol}");
    }
}
