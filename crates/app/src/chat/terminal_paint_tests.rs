use notchling_engine::terminal::TerminalScreen;

use super::*;
use crate::chat::terminal::CELL;

fn screen(bytes: &str) -> TerminalScreen {
    let mut s = TerminalScreen::new(4, 40);
    s.feed(bytes.as_bytes());
    s
}

fn at(span: &Span) -> (usize, usize, &str) {
    (span.col, span.cells, span.text.as_str())
}

#[test]
fn a_prompt_glyph_gets_its_own_span_so_the_text_after_it_keeps_its_column() {
    let s = screen("\x1b[32m\u{276f}\x1b[0m clear");
    let row = &s.cells()[0];
    let spans = spans(row);
    assert_eq!(
        spans.iter().map(at).collect::<Vec<_>>(),
        vec![(0, 1, "\u{276f}"), (1, 6, " clear")]
    );
    assert_eq!(s.cursor(), (0, 7));
}

#[test]
fn every_span_starts_on_the_column_of_its_first_cell() {
    let s = screen("ab \x1b[31mcd\x1b[0m\u{65e5}e \u{276f}\u{276f}f");
    let spans = spans(&s.cells()[0]);
    assert_eq!(
        spans.iter().map(at).collect::<Vec<_>>(),
        vec![
            (0, 2, "ab"),
            (3, 2, "cd"),
            (5, 2, "\u{65e5}"),
            (7, 1, "e"),
            (9, 1, "\u{276f}"),
            (10, 1, "\u{276f}"),
            (11, 1, "f"),
        ]
    );
}

#[test]
fn a_wide_character_spans_two_cells_and_its_second_cell_is_skipped() {
    let s = screen("\u{65e5}\u{672c}x");
    let spans = spans(&s.cells()[0]);
    assert_eq!(
        spans.iter().map(at).collect::<Vec<_>>(),
        vec![(0, 2, "\u{65e5}"), (2, 2, "\u{672c}"), (4, 1, "x")]
    );
}

#[test]
fn empty_cells_draw_nothing_unless_they_have_a_background_or_an_underline() {
    let s = screen("a  \x1b[44m  \x1b[0m \x1b[4m \x1b[0m ");
    let spans = spans(&s.cells()[0]);
    assert_eq!(
        spans.iter().map(at).collect::<Vec<_>>(),
        vec![(0, 1, "a"), (3, 2, "  "), (6, 1, " ")]
    );
    assert!(spans[1].style.bg.is_some() && spans[2].style.underline);
}

#[test]
fn the_cursor_after_typed_text_sits_on_the_empty_cell_after_it() {
    let s = screen("\x1b[32m\u{276f}\x1b[0m clear");
    let caret = caret(s.cells(), s.cursor(), true).expect("a cursor");
    assert_eq!((caret.col, caret.cells, caret.text.as_str()), (7, 1, " "));
    assert!(caret.filled);
}

#[test]
fn the_cursor_over_a_character_carries_that_character() {
    let s = screen("\u{276f} clear\x1b[D");
    let caret = caret(s.cells(), s.cursor(), true).expect("a cursor");
    assert_eq!((caret.col, caret.cells, caret.text.as_str()), (6, 1, "r"));
}

#[test]
fn the_cursor_at_the_last_column_stays_over_the_last_character() {
    let mut s = TerminalScreen::new(2, 10);
    s.feed(b"0123456789");
    assert_eq!(s.cursor(), (0, 9));
    let caret = caret(s.cells(), s.cursor(), true).expect("a cursor");
    assert_eq!((caret.col, caret.text.as_str()), (9, "9"));
}

#[test]
fn the_cursor_on_a_wide_character_covers_both_cells() {
    let s = screen("\u{65e5}\u{672c}\x1b[2D");
    let caret = caret(s.cells(), s.cursor(), true).expect("a cursor");
    assert_eq!(
        (caret.col, caret.cells, caret.text.as_str()),
        (2, 2, "\u{672c}")
    );
}

#[test]
fn the_cursor_on_the_second_cell_of_a_wide_character_moves_to_its_first() {
    let s = screen("x\u{65e5}y");
    let caret = caret(s.cells(), (0, 2), true).expect("a cursor");
    assert_eq!(
        (caret.col, caret.cells, caret.text.as_str()),
        (1, 2, "\u{65e5}")
    );
}

#[test]
fn an_unfocused_cursor_is_an_outline_and_a_bold_cell_keeps_its_style() {
    let s = screen("\x1b[1mB\x1b[0m\x1b[D");
    let focused = caret(s.cells(), s.cursor(), true).expect("a cursor");
    let blurred = caret(s.cells(), s.cursor(), false).expect("a cursor");
    assert!(focused.filled && !blurred.filled);
    assert!(focused.style.bold);
}

#[test]
fn a_cursor_outside_the_screen_is_none_or_clamped_to_the_row() {
    let s = screen("ab");
    assert_eq!(caret(s.cells(), (9, 0), true), None);
    let caret = caret(s.cells(), (0, 99), true).expect("a cursor");
    assert_eq!(caret.col, 39);
    assert_eq!(super::caret(&[], (0, 0), true), None);
}

#[test]
fn a_span_is_as_wide_as_the_cells_it_holds() {
    let s = screen("ab\u{65e5}");
    let widths: Vec<f32> = spans(&s.cells()[0])
        .iter()
        .map(|s| s.cells as f32 * CELL)
        .collect();
    assert_eq!(widths, vec![2.0 * CELL, 2.0 * CELL]);
}

#[test]
fn the_cursor_color_has_a_contrast_color_in_both_appearances() {
    use notchling_kit::{Appearance, Ink};
    for a in [Appearance::Light, Appearance::Dark] {
        assert_eq!(Ink::Working.color(a), Ink::Guide.color(a));
    }
}
