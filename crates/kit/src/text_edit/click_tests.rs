use super::*;

fn selected(e: &TextEdit) -> &str {
    e.selected_text()
}

#[test]
fn one_click_puts_the_caret_where_it_landed() {
    let mut e = TextEdit::new("hello world");
    let press = e.press_at(3, 1, false);
    assert_eq!(e.selection(), 3..3);
    assert_eq!(press.anchor, 3..3);
    assert_eq!(press.by, Unit::Grapheme);
}

#[test]
fn a_click_past_the_end_lands_at_the_end() {
    let mut e = TextEdit::new("abc");
    e.press_at(99, 1, false);
    assert_eq!(e.selection(), 3..3);
}

#[test]
fn shift_click_extends_from_the_caret_and_only_for_a_single_click() {
    let mut e = TextEdit::new("hello world");
    e.move_to(2);
    e.press_at(8, 1, true);
    assert_eq!(e.selection(), 2..8);
    e.move_to(2);
    e.press_at(8, 2, true);
    assert_eq!(selected(&e), "world");
}

#[test]
fn a_double_click_takes_the_word_under_it() {
    let mut e = TextEdit::new("hello big world");
    let press = e.press_at(7, 2, false);
    assert_eq!(selected(&e), "big");
    assert_eq!(press.by, Unit::Word);
    e.press_at(0, 2, false);
    assert_eq!(selected(&e), "hello");
    e.press_at(15, 2, false);
    assert_eq!(selected(&e), "world");
}

#[test]
fn a_double_click_on_a_space_or_punctuation_takes_that_run() {
    let mut e = TextEdit::new("one, two");
    e.press_at(3, 2, false);
    assert_eq!(selected(&e), ",");
    e.press_at(4, 2, false);
    assert_eq!(selected(&e), " ");
}

#[test]
fn a_double_click_keeps_a_word_with_accents_whole() {
    let mut e = TextEdit::new("xin chào bạn");
    e.press_at(6, 2, false);
    assert_eq!(selected(&e), "chào");
}

#[test]
fn a_triple_click_takes_the_line_and_not_the_next_one() {
    let mut e = TextEdit::new("first line\nsecond line");
    let press = e.press_at(14, 3, false);
    assert_eq!(selected(&e), "second line");
    assert_eq!(press.by, Unit::Line);
    e.press_at(2, 3, false);
    assert_eq!(selected(&e), "first line");
    e.press_at(2, 7, false);
    assert_eq!(selected(&e), "first line");
}

#[test]
fn a_zero_click_count_is_a_single_click() {
    assert_eq!(unit_for(0), Unit::Grapheme);
    assert_eq!(unit_for(1), Unit::Grapheme);
    assert_eq!(unit_for(2), Unit::Word);
    assert_eq!(unit_for(3), Unit::Line);
}

#[test]
fn dragging_from_a_click_selects_between_the_two_points_either_way() {
    let mut e = TextEdit::new("hello world");
    let press = e.press_at(2, 1, false);
    e.drag_to(&press, 8);
    assert_eq!(e.selection(), 2..8);
    assert!(!e.is_reversed());
    e.drag_to(&press, 0);
    assert_eq!(e.selection(), 0..2);
    assert!(e.is_reversed());
    e.drag_to(&press, 2);
    assert_eq!(e.selection(), 2..2);
}

#[test]
fn dragging_after_a_double_click_grows_by_whole_words() {
    let mut e = TextEdit::new("alpha beta gamma delta");
    let press = e.press_at(8, 2, false);
    assert_eq!(selected(&e), "beta");
    e.drag_to(&press, 13);
    assert_eq!(selected(&e), "beta gamma");
    e.drag_to(&press, 2);
    assert_eq!(selected(&e), "alpha beta");
    assert!(e.is_reversed());
    e.drag_to(&press, 7);
    assert_eq!(selected(&e), "beta");
}

#[test]
fn dragging_after_a_triple_click_grows_by_whole_lines() {
    let mut e = TextEdit::new("one\ntwo\nthree");
    let press = e.press_at(5, 3, false);
    assert_eq!(selected(&e), "two");
    e.drag_to(&press, 10);
    assert_eq!(selected(&e), "two\nthree");
    e.drag_to(&press, 0);
    assert_eq!(selected(&e), "one\ntwo");
}

#[test]
fn a_selection_made_by_click_is_what_copy_and_cut_take() {
    let mut e = TextEdit::new("hello big world");
    e.press_at(7, 2, false);
    assert_eq!(e.selected_text(), "big");
    assert_eq!(e.cut().as_deref(), Some("big"));
    assert_eq!(e.text(), "hello  world");
}

#[test]
fn select_all_then_cut_empties_the_box_and_undo_brings_it_back() {
    let mut e = TextEdit::new("one two");
    e.select_all();
    assert_eq!(e.selected_text(), "one two");
    assert_eq!(e.cut().as_deref(), Some("one two"));
    assert!(e.is_empty());
    assert!(e.undo());
    assert_eq!(e.text(), "one two");
}

#[test]
fn shift_with_the_arrow_keys_grows_and_shrinks_the_selection() {
    let mut e = TextEdit::new("hello world");
    e.move_to(0);
    e.move_right(Unit::Grapheme, true);
    e.move_right(Unit::Grapheme, true);
    assert_eq!(selected(&e), "he");
    e.move_right(Unit::Word, true);
    assert_eq!(selected(&e), "hello");
    e.move_left(Unit::Grapheme, true);
    assert_eq!(selected(&e), "hell");
    e.move_to(11);
    e.move_left(Unit::Word, true);
    assert_eq!(selected(&e), "world");
    e.move_left(Unit::Line, true);
    assert_eq!(selected(&e), "hello world");
}
