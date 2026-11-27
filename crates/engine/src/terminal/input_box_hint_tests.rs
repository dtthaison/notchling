use super::input_box::{
    cursor_drawn, hint_shown, read, read_shaded, typed_text_shaded, Fill, Input,
};
use super::screen::{Shade, TerminalScreen};

const RULE: &str = "────────────────────────────────────────────────";
const HINT: &str = "Try \"refactor parser.rs\"";
const INVERT: &str = "\x1b[7m";
const PLAIN: &str = "\x1b[27m";
const DIM: &str = "\x1b[2m";
const BRIGHT: &str = "\x1b[22m";

struct Drawn {
    lines: Vec<String>,
    shades: Vec<Vec<Shade>>,
    cursor: Option<(usize, usize)>,
}

impl Drawn {
    fn input(&self) -> Input {
        read_shaded(&self.lines, self.cursor, &self.shades).expect("a box")
    }

    fn hint(&self) -> bool {
        hint_shown(&self.lines, &self.shades)
    }

    fn own_cursor(&self) -> bool {
        cursor_drawn(&self.lines, &self.shades)
    }

    fn text(&self) -> Option<String> {
        typed_text_shaded(&self.lines, self.cursor, &self.shades)
    }

    fn held_text(&self) -> bool {
        !self.input().empty && !self.hint()
    }
}

fn dim_hint(hint: &str) -> String {
    format!("{DIM}{hint}{BRIGHT}")
}

fn cursor_hint(hint: &str) -> String {
    let mut chars = hint.chars();
    let first = chars.next().unwrap_or(' ');
    format!("{INVERT}{first}{PLAIN}{}", dim_hint(chars.as_str()))
}

fn cursor_cell() -> String {
    format!("{INVERT} {PLAIN}")
}

fn drawn(rows: &[&str], cursor_shown: Option<(usize, usize)>) -> Drawn {
    let mut screen = TerminalScreen::new(8, 60);
    let mut frame = String::from("\x1b[H");
    let mut put = |row: usize, text: &str| frame.push_str(&format!("\x1b[{};1H{text}", row + 1));
    put(0, "⏺ ok");
    put(1, RULE);
    for (n, row) in rows.iter().enumerate() {
        let mark = if n == 0 { "❯" } else { " " };
        put(2 + n, &format!("{mark}\u{a0}{row}"));
    }
    put(2 + rows.len(), RULE);
    frame.push_str(&match cursor_shown {
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

#[test]
fn a_hint_drawn_with_claude_codes_own_cursor_is_an_empty_box() {
    let screen = drawn(&[&cursor_hint(HINT)], None);
    assert_eq!(screen.cursor, None);
    assert_eq!(screen.lines[2], format!("❯\u{a0}{HINT}"));
    let input = screen.input();
    assert!(input.empty, "{input:?}");
    assert_eq!(input.held, ["❯"], "the hint is not text in the box");
    assert!(screen.hint() && screen.own_cursor());
    assert_eq!(input.fill("hi"), Fill::Empty);
    assert!(!input.holds("hi"));
    assert_eq!(screen.text(), None);
}

#[test]
fn a_hint_drawn_all_faint_is_an_empty_box_whether_or_not_the_cursor_shows() {
    let unfocused = drawn(&[&dim_hint(HINT)], None);
    assert!(unfocused.input().empty && unfocused.hint());
    assert!(!unfocused.own_cursor(), "no cursor of Claude Code's own");

    let native = drawn(&[&dim_hint(HINT)], Some((2, 2)));
    assert!(native.input().empty && native.hint());
    assert_eq!(native.input().fill("hi"), Fill::Empty);
}

#[test]
fn a_hint_that_wraps_is_an_empty_box() {
    let screen = drawn(
        &[
            &cursor_hint("Try \"write a test for a very long file name\""),
            &dim_hint("  and the next row of it"),
        ],
        None,
    );
    assert_eq!(screen.lines.len(), 8);
    assert!(screen.input().empty && screen.hint());
    assert_eq!(screen.text(), None);
}

#[test]
fn a_hint_is_never_the_message_even_when_the_words_are_the_same() {
    let input = drawn(&[&cursor_hint(HINT)], None).input();
    assert!(!input.shows(HINT));
    assert_eq!(input.fill(HINT), Fill::Empty);
    let by_cursor = read(
        &[
            "⏺ ok".to_string(),
            RULE.to_string(),
            format!("❯\u{a0}{HINT}"),
            RULE.to_string(),
        ],
        Some((2, 2)),
    )
    .unwrap();
    assert!(
        by_cursor.shows(HINT),
        "with no styles the words are all there is"
    );
}

#[test]
fn the_same_words_typed_by_a_person_are_text() {
    let typed = format!("{HINT}{}", cursor_cell());
    let screen = drawn(&[&typed], None);
    assert!(screen.held_text());
    assert!(screen.own_cursor());
    assert_eq!(screen.input().fill("hi"), Fill::Foreign);
    assert_eq!(screen.text().as_deref(), Some(HINT));
}

#[test]
fn text_with_the_cursor_on_its_first_letter_is_still_text() {
    let home = format!("{INVERT}T{PLAIN}ry \"refactor parser.rs\"");
    assert!(drawn(&[&home], None).held_text());
    let one = format!("{INVERT}y{PLAIN}");
    assert!(
        drawn(&[&one], None).held_text(),
        "one letter under the cursor"
    );
}

#[test]
fn a_hint_with_anything_plain_in_it_is_not_a_hint() {
    let overtyped = format!("x{}", dim_hint("ry \"refactor parser.rs\""));
    assert!(drawn(&[&overtyped], None).held_text());
    let beside = format!("{}and mine", dim_hint("Try \"refactor parser.rs\" "));
    assert!(drawn(&[&beside], None).held_text());
    let two = format!("{}x", cursor_hint("Try"));
    assert!(drawn(&[&two], None).held_text());
    assert!(drawn(&[&cursor_hint(HINT), "my own words"], None).held_text());
}

#[test]
fn a_cursor_inside_the_hint_is_not_the_first_letter() {
    let inside = format!(
        "{}{INVERT}r{PLAIN}{}",
        dim_hint("Try \""),
        dim_hint("efactor\"")
    );
    assert!(drawn(&[&inside], None).held_text());
}

#[test]
fn a_hint_with_no_styling_reads_as_text_as_before() {
    let screen = drawn(&[HINT], None);
    assert!(screen.held_text());
    let blind = read(&screen.lines, screen.cursor).unwrap();
    assert_eq!(screen.input(), blind);
}

#[test]
fn shades_that_do_not_cover_the_box_say_nothing() {
    let screen = drawn(&[&cursor_hint(HINT)], None);
    for shades in [Vec::new(), screen.shades[..2].to_vec()] {
        let input = read_shaded(&screen.lines, None, &shades).unwrap();
        assert!(!input.empty, "{input:?}");
        assert!(!hint_shown(&screen.lines, &shades));
        assert!(!cursor_drawn(&screen.lines, &shades));
    }
}

#[test]
fn an_empty_box_with_only_claude_codes_cursor_is_empty_and_drawn() {
    let screen = drawn(&[&cursor_cell()], None);
    assert!(screen.input().empty && !screen.hint() && screen.own_cursor());
    let none = drawn(&[""], None);
    assert!(none.input().empty && !none.own_cursor());
}

#[test]
fn the_screen_tells_faint_text_and_the_inverse_cell_from_plain_text() {
    let mut screen = TerminalScreen::new(2, 20);
    screen.feed(format!("a{INVERT}b{PLAIN}{DIM}c{BRIGHT}d{DIM}{INVERT}e").as_bytes());
    let row = &screen.shades()[0];
    assert_eq!(
        row[..5],
        [
            Shade::Plain,
            Shade::Cursor,
            Shade::Faint,
            Shade::Plain,
            Shade::Cursor
        ]
    );
    assert_eq!(row.len(), 20);
    assert_eq!(screen.lines()[0], "abcde");
}

#[test]
fn shades_line_up_with_the_lines_text_through_wide_and_combined_characters() {
    let mut screen = TerminalScreen::new(1, 20);
    screen.feed(format!("日{DIM}e\u{301}x{BRIGHT}y").as_bytes());
    let line = screen.lines().remove(0);
    let row = &screen.shades()[0];
    let chars: Vec<char> = line.chars().collect();
    assert_eq!(chars, ['日', 'e', '\u{301}', 'x', 'y']);
    assert_eq!(
        row[..chars.len()],
        [
            Shade::Plain,
            Shade::Faint,
            Shade::Faint,
            Shade::Faint,
            Shade::Plain
        ]
    );
}
