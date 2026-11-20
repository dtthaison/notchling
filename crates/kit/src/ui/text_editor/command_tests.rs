use super::*;

#[test]
fn copy_and_cut_need_a_selection_and_the_rest_do_not() {
    assert_eq!(route(Command::Copy, true, false), Route::Pass);
    assert_eq!(route(Command::Cut, true, false), Route::Pass);
    assert_eq!(route(Command::Copy, false, false), Route::Run);
    assert_eq!(route(Command::Cut, false, false), Route::Run);
    for command in [
        Command::Paste,
        Command::SelectAll,
        Command::Undo,
        Command::Redo,
    ] {
        assert_eq!(route(command, true, false), Route::Run);
        assert_eq!(route(command, false, false), Route::Run);
        assert_eq!(route(command, false, true), Route::Run);
    }
}

#[test]
fn a_masked_field_swallows_copy_and_cut_whatever_is_selected() {
    for empty in [true, false] {
        assert_eq!(route(Command::Copy, empty, true), Route::Swallow);
        assert_eq!(route(Command::Cut, empty, true), Route::Swallow);
    }
}

#[test]
fn a_single_line_paste_loses_newlines_and_control_characters() {
    assert_eq!(single_line_text("a\nb\r\nc", false), "a b  c");
    assert_eq!(single_line_text("a\tb\u{1b}c\u{7f}", false), "abc");
    assert_eq!(single_line_text("pass\n", true), "pass");
    assert_eq!(
        single_line_text("p\u{e9}w \u{4e2d}", true),
        "p\u{e9}w \u{4e2d}"
    );
}

fn keys(platform: bool, alt: bool) -> Modifiers {
    Modifiers {
        platform,
        alt,
        ..Modifiers::default()
    }
}

#[test]
fn option_and_command_make_a_word_or_line_move_that_a_masked_field_turns_into_a_document_move() {
    assert_eq!(TextEditor::unit(&keys(false, false), false), Unit::Grapheme);
    assert_eq!(TextEditor::unit(&keys(false, true), false), Unit::Word);
    assert_eq!(TextEditor::unit(&keys(true, false), false), Unit::Line);
    assert_eq!(TextEditor::unit(&keys(false, false), true), Unit::Grapheme);
    assert_eq!(TextEditor::unit(&keys(false, true), true), Unit::Document);
    assert_eq!(TextEditor::unit(&keys(true, false), true), Unit::Document);
}

#[test]
fn a_shortcut_needs_command_alone_and_takes_either_case_of_the_letter() {
    let command = Modifiers {
        platform: true,
        ..Modifiers::default()
    };
    assert!(is_shortcut(&command));
    assert!(!is_shortcut(&keys(false, false)));
    assert!(!is_shortcut(&Modifiers {
        platform: true,
        alt: true,
        ..Modifiers::default()
    }));
    assert!(!is_shortcut(&Modifiers {
        platform: true,
        control: true,
        ..Modifiers::default()
    }));
    assert!(is_shortcut(&Modifiers {
        platform: true,
        shift: true,
        ..Modifiers::default()
    }));
    assert_eq!(shortcut_letter("C"), "c");
    assert_eq!(shortcut_letter("v"), "v");
    assert_eq!(shortcut_letter("enter"), "enter");
}
