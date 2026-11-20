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
