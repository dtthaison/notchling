use super::*;

fn edit(text: &str) -> TextEdit {
    TextEdit::new(text)
}

#[test]
fn starts_with_the_caret_at_the_end_and_nothing_selected() {
    let e = edit("hello");
    assert_eq!(e.text(), "hello");
    assert_eq!(e.caret(), 5);
    assert_eq!(e.selection(), 5..5);
    assert!(!e.is_reversed());
    assert_eq!(e.marked(), None);
    assert!(edit("").is_empty());
}

#[test]
fn set_text_moves_the_caret_to_the_end_and_forgets_history() {
    let mut e = edit("");
    e.insert("typed");
    e.set_text("a draft");
    assert_eq!(e.caret(), 7);
    assert!(!e.undo(), "a new text has no past");
    assert_eq!(e.text(), "a draft");
}

#[test]
fn insert_replaces_the_selection_and_puts_the_caret_after() {
    let mut e = edit("hello world");
    e.set_selection(6, 11);
    e.insert("there");
    assert_eq!(e.text(), "hello there");
    assert_eq!(e.selection(), 11..11);
    e.move_to(0);
    e.insert("oh, ");
    assert_eq!(e.text(), "oh, hello there");
    assert_eq!(e.caret(), 4);
}

#[test]
fn grapheme_moves_step_over_combining_marks_and_emoji_sequences() {
    let mut e = edit("e\u{301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}t");
    let len = e.text().len();
    assert_eq!(e.caret(), len);
    e.move_left(Unit::Grapheme, false);
    assert_eq!(e.caret(), len - 1);
    e.move_left(Unit::Grapheme, false);
    assert_eq!(e.caret(), 3, "the whole family is one step");
    e.move_left(Unit::Grapheme, false);
    assert_eq!(e.caret(), 0, "e and its accent are one step");
    e.move_left(Unit::Grapheme, false);
    assert_eq!(e.caret(), 0, "stays at the start");
    e.move_right(Unit::Grapheme, false);
    assert_eq!(e.caret(), 3);
    e.move_right(Unit::Grapheme, false);
    assert_eq!(e.caret(), len - 1);
    e.move_right(Unit::Grapheme, false);
    e.move_right(Unit::Grapheme, false);
    assert_eq!(e.caret(), len, "stays at the end");
}

#[test]
fn word_moves_skip_spaces_and_punctuation_the_way_option_arrows_do() {
    let mut e = edit("hello  world, foo");
    e.move_to(0);
    e.move_right(Unit::Word, false);
    assert_eq!(e.caret(), 5);
    e.move_right(Unit::Word, false);
    assert_eq!(e.caret(), 12);
    e.move_right(Unit::Word, false);
    assert_eq!(e.caret(), 17);
    e.move_right(Unit::Word, false);
    assert_eq!(e.caret(), 17);
    e.move_left(Unit::Word, false);
    assert_eq!(e.caret(), 14);
    e.move_left(Unit::Word, false);
    assert_eq!(e.caret(), 7);
    e.move_left(Unit::Word, false);
    assert_eq!(e.caret(), 0);
    e.move_left(Unit::Word, false);
    assert_eq!(e.caret(), 0);
    let mut v = edit("xin chào việt nam");
    v.move_to(0);
    v.move_right(Unit::Word, false);
    v.move_right(Unit::Word, false);
    assert_eq!(&v.text()[..v.caret()], "xin chào");
}

#[test]
fn line_moves_go_to_the_paragraph_edges_and_document_moves_to_the_ends() {
    let mut e = edit("first line\nsecond line\nthird");
    e.move_to(14);
    e.move_left(Unit::Line, false);
    assert_eq!(e.caret(), 11);
    e.move_right(Unit::Line, false);
    assert_eq!(e.caret(), 22);
    e.move_right(Unit::Line, false);
    assert_eq!(e.caret(), 22, "stays at the end of its paragraph");
    e.move_left(Unit::Document, false);
    assert_eq!(e.caret(), 0);
    e.move_right(Unit::Document, false);
    assert_eq!(e.caret(), e.text().len());
    assert_eq!(e.line_range_at(14), 11..22);
    assert_eq!(e.line_range_at(0), 0..10);
    assert_eq!(e.line_range_at(28), 23..28);
}

#[test]
fn shift_moves_extend_the_selection_and_plain_moves_collapse_it() {
    let mut e = edit("hello world");
    e.move_to(6);
    e.move_right(Unit::Word, true);
    assert_eq!(e.selection(), 6..11);
    assert!(!e.is_reversed());
    assert_eq!(e.selected_text(), "world");
    e.move_left(Unit::Grapheme, false);
    assert_eq!(e.selection(), 6..6, "Left collapses to the start");
    e.move_right(Unit::Word, true);
    e.move_right(Unit::Grapheme, false);
    assert_eq!(e.selection(), 11..11, "Right collapses to the end");
    e.move_left(Unit::Word, true);
    assert_eq!(e.selection(), 6..11);
    assert!(e.is_reversed(), "the caret is at the start");
    assert_eq!(e.caret(), 6);
    e.move_left(Unit::Grapheme, true);
    assert_eq!(e.selection(), 5..11);
    e.move_to(5);
    e.move_right(Unit::Grapheme, true);
    e.move_left(Unit::Grapheme, true);
    e.move_left(Unit::Grapheme, true);
    assert_eq!(e.selection(), 4..5);
    assert!(e.is_reversed());
}

#[test]
fn deleting_takes_a_grapheme_a_word_or_the_line_and_a_selection_first() {
    let mut e = edit("hello world");
    e.delete_backward(Unit::Grapheme);
    assert_eq!(e.text(), "hello worl");
    e.delete_backward(Unit::Word);
    assert_eq!(e.text(), "hello ");
    e.insert("there\nagain");
    e.delete_backward(Unit::Line);
    assert_eq!(e.text(), "hello there\n");
    e.delete_backward(Unit::Grapheme);
    assert_eq!(e.text(), "hello there");
    e.move_to(0);
    e.delete_forward(Unit::Grapheme);
    assert_eq!(e.text(), "ello there");
    e.delete_forward(Unit::Word);
    assert_eq!(e.text(), " there");
    e.delete_forward(Unit::Line);
    assert_eq!(e.text(), "");
    e.delete_forward(Unit::Grapheme);
    e.delete_backward(Unit::Grapheme);
    assert_eq!(e.text(), "", "nothing to delete is fine");
    let mut s = edit("one two three");
    s.set_selection(4, 7);
    s.delete_backward(Unit::Word);
    assert_eq!(
        s.text(),
        "one  three",
        "a selection goes first, whatever the unit"
    );
    assert_eq!(s.caret(), 4);
    let mut c = edit("e\u{301}x");
    c.move_to(0);
    c.delete_forward(Unit::Grapheme);
    assert_eq!(c.text(), "x", "the accent goes with its letter");
}

#[test]
fn select_all_word_and_line_at_a_point() {
    let mut e = edit("hello world\nnext line");
    e.select_all();
    assert_eq!(e.selection(), 0..21);
    e.select_word_at(7);
    assert_eq!(e.selected_text(), "world");
    assert_eq!(e.word_range_at(0), 0..5);
    assert_eq!(e.word_range_at(5), 5..6, "the space between is its own run");
    assert_eq!(e.word_range_at(21), 17..21, "the end picks the last word");
    e.select_line_at(14);
    assert_eq!(e.selected_text(), "next line");
    e.select_line_at(3);
    assert_eq!(e.selected_text(), "hello world");
}

#[test]
fn cut_takes_the_selection_and_paste_is_its_own_undo_step() {
    let mut e = edit("hello world");
    assert_eq!(e.cut(), None, "nothing selected, nothing cut");
    e.set_selection(0, 6);
    assert_eq!(e.cut().as_deref(), Some("hello "));
    assert_eq!(e.text(), "world");
    assert_eq!(e.caret(), 0);
    e.paste("big ");
    e.paste("wide ");
    assert_eq!(e.text(), "big wide world");
    assert!(e.undo());
    assert_eq!(e.text(), "big world");
    assert!(e.undo());
    assert_eq!(e.text(), "world");
    assert!(e.undo());
    assert_eq!(e.text(), "hello world");
    assert_eq!(e.selection(), 0..6, "undo brings the selection back");
}

#[test]
fn typing_is_one_undo_group_until_the_caret_moves_or_the_kind_changes() {
    let mut e = edit("");
    for c in ["h", "e", "y"] {
        e.insert(c);
    }
    assert!(e.undo());
    assert_eq!(e.text(), "", "three keystrokes, one step");
    assert!(e.redo());
    assert_eq!(e.text(), "hey");
    assert!(!e.redo());
    e.insert(" you");
    e.move_to(0);
    e.insert("oh ");
    assert!(e.undo());
    assert_eq!(e.text(), "hey you", "a caret move closes the group");
    assert_eq!(e.caret(), 0);
    assert!(e.undo());
    assert_eq!(e.text(), "hey", "so does an undo or redo");
    assert!(e.undo());
    assert_eq!(e.text(), "");
    assert!(!e.undo());
    e.insert("abc");
    e.delete_backward(Unit::Grapheme);
    e.delete_backward(Unit::Grapheme);
    assert_eq!(e.text(), "a");
    assert!(e.undo());
    assert_eq!(e.text(), "abc");
    assert!(e.undo());
    assert_eq!(e.text(), "");
    assert!(e.redo());
    e.insert("z");
    assert!(!e.redo(), "a new edit forgets what was undone");
    assert_eq!(e.text(), "abcz");
}

#[test]
fn utf16_offsets_map_both_ways_over_astral_characters() {
    let e = edit("a😀b");
    assert_eq!(e.text().len(), 6);
    assert_eq!(e.utf8_to_utf16(0), 0);
    assert_eq!(e.utf8_to_utf16(1), 1);
    assert_eq!(e.utf8_to_utf16(5), 3);
    assert_eq!(e.utf8_to_utf16(6), 4);
    assert_eq!(e.utf16_to_utf8(0), 0);
    assert_eq!(e.utf16_to_utf8(1), 1);
    assert_eq!(e.utf16_to_utf8(3), 5);
    assert_eq!(e.utf16_to_utf8(4), 6);
    assert_eq!(e.utf16_to_utf8(99), 6, "past the end clamps");
    assert_eq!(
        e.utf16_to_utf8(2),
        5,
        "inside a surrogate pair rounds up to the next character"
    );
    let (text, actual) = e.text_for_range_utf16(1..3);
    assert_eq!(text, "😀");
    assert_eq!(actual, 1..3);
    let (text, actual) = e.text_for_range_utf16(3..40);
    assert_eq!(text, "b");
    assert_eq!(actual, 3..4);
}

#[test]
fn a_telex_composition_ends_with_viet() {
    let mut e = edit("");
    let steps = ["v", "vi", "vie", "viê", "việ", "việt"];
    for word in steps {
        let caret16 = word.encode_utf16().count();
        e.replace_and_mark(None, word, Some(caret16..caret16));
        assert_eq!(e.text(), word);
        assert_eq!(e.marked(), Some(0..word.len()), "the word stays marked");
        assert_eq!(e.caret(), word.len());
        assert_eq!(e.marked_utf16(), Some(0..caret16));
    }
    e.replace_text(None, "việt");
    assert_eq!(e.text(), "việt");
    assert_eq!(e.marked(), None, "a commit unmarks");
    assert_eq!(e.caret(), "việt".len());
    e.replace_text(None, " ");
    e.replace_and_mark(None, "n", Some(1..1));
    e.replace_and_mark(None, "na", Some(2..2));
    e.replace_and_mark(None, "nam", Some(3..3));
    assert_eq!(e.text(), "việt nam");
    assert_eq!(e.marked(), Some("việt ".len().."việt nam".len()));
    e.replace_text(None, "nam");
    assert_eq!(e.text(), "việt nam");
    assert_eq!(e.marked(), None);
    assert!(e.undo());
    assert_eq!(e.text(), "", "a composition is typing: one undo step");
}

#[test]
fn marked_text_replaces_the_selection_first_and_an_explicit_range_wins() {
    let mut e = edit("hello world");
    e.set_selection(6, 11);
    e.replace_and_mark(None, "w", Some(1..1));
    assert_eq!(e.text(), "hello w");
    assert_eq!(e.marked(), Some(6..7));
    e.replace_and_mark(Some(0..5), "HELLO", None);
    assert_eq!(e.text(), "HELLO w");
    assert_eq!(e.marked(), Some(0..5));
    assert_eq!(
        e.caret(),
        5,
        "no selection given: the caret follows the text"
    );
    e.unmark();
    assert_eq!(e.marked(), None);
    assert_eq!(e.text(), "HELLO w");
    let mut m = edit("x");
    m.replace_and_mark(None, "a😀b", Some(1..3));
    assert_eq!(m.text(), "xa😀b");
    assert_eq!(m.selection(), 2..6);
    assert!(!m.is_reversed());
    m.replace_and_mark(None, "", None);
    assert_eq!(m.text(), "x");
    assert_eq!(m.marked(), None);
}

#[test]
fn replace_text_with_a_range_edits_there_and_leaves_the_caret_after_it() {
    let mut e = edit("a😀b");
    e.replace_text(Some(1..3), "Z");
    assert_eq!(e.text(), "aZb");
    assert_eq!(e.caret(), 2);
    assert_eq!(e.selection_utf16().range, 2..2);
    assert!(!e.selection_utf16().reversed);
    e.set_selection(3, 1);
    let sel = e.selection_utf16();
    assert_eq!(sel.range, 1..3);
    assert!(sel.reversed);
}

#[test]
fn set_selection_clamps_to_the_text_and_character_boundaries() {
    let mut e = edit("a😀b");
    e.set_selection(2, 100);
    assert_eq!(
        e.selection(),
        1..6,
        "inside the emoji snaps back to its start"
    );
    e.move_to(3);
    assert_eq!(e.caret(), 1);
}

#[test]
fn return_sends_and_shift_or_option_return_adds_a_line() {
    assert_eq!(return_key(false, false), ReturnKey::Submit);
    assert_eq!(return_key(true, false), ReturnKey::Newline);
    assert_eq!(return_key(false, true), ReturnKey::Newline);
    assert_eq!(return_key(true, true), ReturnKey::Newline);
}

#[test]
fn history_keeps_a_bounded_number_of_steps() {
    let mut e = edit("");
    for i in 0..(history::LIMIT + 20) {
        e.paste(&i.to_string());
    }
    let mut undone = 0;
    while e.undo() {
        undone += 1;
    }
    assert_eq!(undone, history::LIMIT);
}

#[test]
fn a_commit_that_lands_after_the_box_was_cleared_is_dropped() {
    let mut e = edit("xin chào ");
    e.replace_and_mark(None, "v", Some(1..1));
    e.replace_and_mark(None, "việt", Some(4..4));
    assert_eq!(e.text(), "xin chào việt");
    e.set_text("");
    assert_eq!(e.marked(), None);
    e.replace_text(None, "việt");
    assert_eq!(e.text(), "", "the late commit is dropped");
    e.replace_text(None, "việt");
    assert_eq!(e.text(), "việt");
    let mut plain = edit("sent");
    plain.set_text("");
    plain.replace_text(None, "s");
    assert_eq!(plain.text(), "s");
    let mut other = edit("");
    other.replace_and_mark(None, "ab", Some(2..2));
    other.set_text("");
    other.replace_text(None, "x");
    other.replace_text(None, "ab");
    assert_eq!(other.text(), "xab");
    let mut again = edit("");
    again.replace_and_mark(None, "ab", Some(2..2));
    again.set_text("");
    again.replace_and_mark(None, "c", Some(1..1));
    assert_eq!(again.text(), "c");
    assert_eq!(again.marked(), Some(0..1));
    again.replace_text(None, "ab");
    assert_eq!(
        again.text(),
        "ab",
        "the commit replaces the live composition, as always"
    );
}
