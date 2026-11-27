use super::input_box::{
    cursor_drawn, hint_shown, prompt_row, read, read_shaded, stray_highlight, typed_text_shaded,
    Fill, Input,
};
use super::screen::{Shade, TerminalScreen};

const RULE: &str =
    "\x1b[38;2;136;136;136m────────────────────────────────────────────────────────\x1b[39m";
const MARK: &str = "❯\u{a0}";
const REAL_HINT: &str = "\x1b[2mTry\x1b[7G\"fix\x1b[12Gtypecheck\x1b[22Gerrors\"\x1b[22m";
const REAL_CHIP: &str = "[Pasted text #1 +30 lines]";
const CURSOR: &str = "\x1b[7m \x1b[27m";

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

    fn is_text(&self) -> bool {
        !self.input().empty && !self.hint() && self.text().is_some()
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

fn row(text: &str) -> String {
    format!("{MARK}{text}")
}

#[test]
fn a_hint_drawn_the_way_claude_code_2_1_328_does_is_an_empty_box() {
    let screen = drawn(&[&row(REAL_HINT)], Some((2, 2)));
    assert_eq!(
        screen.lines[2],
        format!("{MARK}Try \"fix typecheck errors\"")
    );
    assert!(screen.input().empty, "{:?}", screen.input());
    assert!(screen.hint());
    assert_eq!(screen.input().fill("hi"), Fill::Empty);
    assert_eq!(screen.text(), None);
}

#[test]
fn a_plain_draft_is_text_even_with_the_cursor_sent_to_its_start() {
    let screen = drawn(&[&row("hello there plain")], Some((2, 2)));
    assert_eq!(screen.cursor, Some((2, 2)));
    assert!(!screen.input().empty, "{:?}", screen.input());
    assert!(screen.is_text());
    assert_eq!(screen.text().as_deref(), Some("hello there plain"));
    let unstyled = read(&screen.lines, screen.cursor).expect("a box");
    assert!(
        unstyled.empty,
        "without styles the start of the box is all there is: {unstyled:?}"
    );
}

#[test]
fn a_draft_with_its_cursor_at_the_end_is_text() {
    let screen = drawn(&[&row("hello there plain")], Some((2, 19)));
    assert!(screen.is_text());
}

#[test]
fn a_pasted_text_chip_drawn_as_claude_code_does_is_text() {
    let chip = drawn(&[&row(REAL_CHIP)], Some((2, 2)));
    assert!(!chip.input().empty && !chip.hint(), "{:?}", chip.input());
    assert_eq!(chip.text().as_deref(), Some(REAL_CHIP));
    let both = drawn(
        &[&row(&format!(
            "{REAL_CHIP} and\x1b[34Gthen\x1b[39Gtyped\x1b[45Gwords"
        ))],
        Some((2, 2)),
    );
    assert!(both.is_text());
    assert!(both.lines[2].contains("[Pasted text #1 +30 lines] and then typed words"));
}

#[test]
fn a_chip_drawn_faint_is_still_text_not_a_hint() {
    for chip in [
        "[Pasted text #1 +30 lines]",
        "[Pasted text #12]",
        "[Image #1]",
        "[Image #3] [Image #4]",
        "[...Truncated text #2 +9 lines...]",
        "[✦ Team setup guide #1 +20 lines]",
        "[Image]",
    ] {
        let faint = drawn(&[&row(&format!("\x1b[2m{chip}\x1b[22m"))], Some((2, 2)));
        assert!(!faint.hint(), "{chip}");
        assert!(!faint.input().empty, "{chip}: {:?}", faint.input());
        assert!(faint.is_text(), "{chip}");
        let inverted = drawn(
            &[&row(&format!(
                "\x1b[7m[\x1b[27m\x1b[2m{}\x1b[22m",
                &chip[1..]
            ))],
            None,
        );
        assert!(!inverted.hint(), "{chip}");
        assert!(!inverted.input().empty, "{chip}: {:?}", inverted.input());
    }
}

#[test]
fn a_chip_after_a_hint_looking_word_is_text() {
    let screen = drawn(
        &[&row("\x1b[2mTry \"fix it\" [Image #1]\x1b[22m")],
        Some((2, 2)),
    );
    assert!(!screen.hint());
    assert!(!screen.input().empty);
}

#[test]
fn square_brackets_that_are_not_a_chip_leave_a_real_hint_alone() {
    let screen = drawn(
        &[&row("\x1b[2mTry \"fix a[0] in parser.rs\"\x1b[22m")],
        Some((2, 2)),
    );
    assert!(screen.hint());
    assert!(screen.input().empty, "{:?}", screen.input());
}

#[test]
fn faint_words_away_from_the_prompt_mark_are_not_a_hint() {
    let far = drawn(
        &[&format!(
            "❯{}\x1b[2mTry \"fix parser.rs\"\x1b[22m",
            " ".repeat(12)
        )],
        Some((2, 2)),
    );
    assert!(!far.hint());
    assert!(!far.input().empty, "{:?}", far.input());
    let below = drawn(
        &[MARK, "\x1b[2mTry \"fix parser.rs\"\x1b[22m"],
        Some((2, 2)),
    );
    assert!(!below.hint());
    assert!(!below.input().empty, "{:?}", below.input());
}

#[test]
fn a_hint_in_a_box_without_a_prompt_mark_is_not_a_hint() {
    let screen = drawn(&["  \x1b[2mTry \"fix parser.rs\"\x1b[22m"], None);
    assert!(!screen.hint());
    assert!(!prompt_row(&screen.lines));
}

#[test]
fn a_hint_that_wraps_onto_a_second_row_is_one_hint() {
    let screen = drawn(
        &[
            &row("\x1b[7mT\x1b[27m\x1b[2mry \"fix the parser and then the\x1b[22m"),
            "  \x1b[2mlexer in parser.rs\"\x1b[22m",
        ],
        None,
    );
    assert!(screen.hint());
    assert!(screen.input().empty, "{:?}", screen.input());
    assert!(screen.own_cursor());
}

#[test]
fn a_hint_with_a_plain_letter_drawn_over_part_of_it_is_text() {
    for over in [
        "\x1b[2mTry \"fix\x1b[22mx\x1b[2m parser.rs\"\x1b[22m",
        "\x1b[2mTry\x1b[22mq\x1b[2m\"fix parser.rs\"\x1b[22m",
        "\x1b[2mTry \"fix parser.rs\"\x1b[22mz",
    ] {
        for cursor in [Some((2, 2)), None] {
            let screen = drawn(&[&row(over)], cursor);
            assert!(!screen.hint(), "{:?}", screen.lines[2]);
            assert!(!screen.input().empty, "{:?}", screen.input());
            assert!(screen.is_text(), "{:?}", screen.lines[2]);
        }
    }
}

#[test]
fn a_hint_with_a_row_erased_between_its_rows_is_text() {
    let screen = drawn(
        &[
            &row("\x1b[2mTry \"fix the parser\x1b[22m"),
            "",
            "  \x1b[2mand the lexer\"\x1b[22m",
        ],
        Some((2, 2)),
    );
    assert!(!screen.hint());
    assert!(!screen.input().empty, "{:?}", screen.input());
}

#[test]
fn a_hint_whose_first_row_was_erased_leaves_the_rest_as_text() {
    let screen = drawn(
        &[MARK, "  \x1b[2mlexer in parser.rs\"\x1b[22m"],
        Some((2, 2)),
    );
    assert!(!screen.hint());
    assert!(!screen.input().empty, "{:?}", screen.input());
}

#[test]
fn a_hint_row_erased_to_nothing_is_a_blank_box() {
    let screen = drawn(&[MARK], Some((2, 2)));
    assert!(screen.input().empty);
    assert!(!screen.hint());
}

#[test]
fn the_sessions_own_cursor_is_one_inverse_cell_in_the_first_row_after_the_mark() {
    let empty = drawn(&[&row(CURSOR)], None);
    assert!(empty.own_cursor());
    let typed = drawn(&[&row(&format!("hello{CURSOR}"))], None);
    assert!(typed.own_cursor());
    let wrapped = drawn(&[&row("hello there"), &format!("  {CURSOR}")], None);
    assert!(!wrapped.own_cursor(), "a later row is not the box's first");
}

#[test]
fn more_than_one_inverse_run_is_not_a_cursor() {
    let two = drawn(&[&row(&format!("{CURSOR}ab{CURSOR}"))], None);
    assert!(!two.own_cursor());
    let one_wide = drawn(&[&row("\x1b[7mab\x1b[27m")], None);
    assert!(one_wide.own_cursor());
    let in_two_rows = drawn(&[&row(CURSOR), &format!("  {CURSOR}")], None);
    assert!(!in_two_rows.own_cursor());
}

#[test]
fn a_picker_highlight_between_the_rules_is_not_a_cursor() {
    let picker = drawn(
        &[
            "  Pick a model",
            "\x1b[7m  1. Opus                 \x1b[27m",
            "  2. Sonnet",
        ],
        None,
    );
    assert!(!picker.own_cursor());
    assert!(!prompt_row(&picker.lines));
    assert!(stray_highlight(&picker.lines, &picker.shades));
    let pointer = drawn(
        &[
            "❯\u{a0}1. Opus",
            "\x1b[7m  2. Sonnet                \x1b[27m",
            "  3. Haiku",
        ],
        None,
    );
    assert!(!pointer.own_cursor());
    assert!(stray_highlight(&pointer.lines, &pointer.shades));
}

#[test]
fn a_diff_highlight_between_the_rules_is_not_a_cursor() {
    let diff = drawn(
        &[
            "  Edit file parser.rs",
            "\x1b[7m- let old = 1;\x1b[27m",
            "\x1b[7m+ let new = 2;\x1b[27m",
        ],
        None,
    );
    assert!(!diff.own_cursor());
    assert!(stray_highlight(&diff.lines, &diff.shades));
    let marked = drawn(
        &[
            "❯\u{a0}\x1b[7m- let old = 1;\x1b[27m",
            "\x1b[7m+ let new = 2;\x1b[27m",
        ],
        None,
    );
    assert!(!marked.own_cursor());
}

#[test]
fn a_plain_box_has_no_stray_highlight_and_a_cursor_run_is_still_found() {
    let plain = drawn(&[&row("hello")], None);
    assert!(!stray_highlight(&plain.lines, &plain.shades));
    let cursor = drawn(&[&row(CURSOR)], None);
    assert!(stray_highlight(&cursor.lines, &cursor.shades));
    assert!(cursor.own_cursor());
}

#[test]
fn shades_that_do_not_line_up_with_the_rows_read_no_hint_and_no_cursor() {
    let screen = drawn(&[&row(REAL_HINT)], Some((2, 2)));
    assert!(!hint_shown(&screen.lines, &[]));
    assert!(!cursor_drawn(&screen.lines, &[]));
    assert!(!hint_shown(&screen.lines, &screen.shades[..2]));
}
