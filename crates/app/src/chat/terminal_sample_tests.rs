use notchling_engine::terminal::TerminalScreen;

use super::*;

fn last_row(how: &str) -> (String, (usize, usize)) {
    let mut screen = TerminalScreen::new(10, 64);
    screen.feed(&sample(how).bytes);
    let lines = screen.lines();
    (lines[2].trim_end().to_owned(), screen.cursor())
}

#[test]
fn the_default_sample_ends_at_the_empty_prompt() {
    let (row, cursor) = last_row("");
    assert_eq!(row, "notchling git:(main) \u{276f}");
    assert_eq!(cursor, (2, 23));
}

#[test]
fn typed_leaves_the_cursor_after_the_text_and_over_puts_it_on_the_last_letter() {
    let (row, cursor) = last_row("typed");
    assert_eq!(row, "notchling git:(main) \u{276f} clear");
    assert_eq!(cursor, (2, 28));
    let (row, cursor) = last_row("over");
    assert_eq!(row, "notchling git:(main) \u{276f} clear");
    assert_eq!(cursor, (2, 27));
}

#[test]
fn wide_puts_the_cursor_on_the_second_wide_character() {
    let (row, cursor) = last_row("wide");
    assert_eq!(row, "notchling git:(main) \u{276f} echo \u{65e5}\u{672c}");
    assert_eq!(cursor, (2, 30));
}

#[test]
fn the_words_pick_the_window_and_focus() {
    let s = sample("over,focus,expanded");
    assert!(s.expanded && s.focus);
    let s = sample("typed");
    assert!(!s.expanded && !s.focus);
}
