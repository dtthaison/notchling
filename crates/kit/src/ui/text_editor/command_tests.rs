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
fn a_secret_field_keeps_copy_and_cut_off_while_it_is_shown_and_a_plain_one_never_has_them_off() {
    let hidden = Veil::hidden_secret();
    let shown = hidden.set(false);
    assert!(hidden.masks() && !shown.masks());
    for veil in [hidden, shown] {
        assert!(veil.is_secret());
        for empty in [true, false] {
            assert_eq!(
                route(Command::Copy, empty, veil.is_secret()),
                Route::Swallow
            );
            assert_eq!(route(Command::Cut, empty, veil.is_secret()), Route::Swallow);
        }
    }
    assert!(shown.set(true).masks());
    let plain = Veil::default();
    assert!(!plain.masks() && !plain.is_secret());
    assert!(!plain.set(false).is_secret());
    assert_eq!(route(Command::Copy, false, plain.is_secret()), Route::Run);
}

#[test]
fn unmasking_a_plain_editor_changes_nothing_so_its_undo_history_stays() {
    assert_eq!(Veil::default().set(false), Veil::default());
    assert_eq!(Veil::default().set(false).set(false), Veil::default());
    let hidden = Veil::hidden_secret();
    assert_ne!(hidden.set(false), hidden);
    assert_eq!(hidden.set(false).set(true), hidden);
    assert_eq!(hidden.set(true), hidden);
}

#[test]
fn a_pasted_newline_is_stripped_from_a_secret_whether_it_is_hidden_or_shown() {
    let hidden = Veil::hidden_secret();
    for veil in [hidden, hidden.set(false)] {
        assert_eq!(veil.single_line("pass\n"), "pass");
        assert_eq!(veil.single_line("a\r\nb"), "ab");
    }
    assert_eq!(Veil::default().single_line("a\nb"), "a b");
}

#[test]
fn a_secret_gives_the_input_client_only_the_text_it_is_composing_whether_hidden_or_shown() {
    let hidden = Veil::hidden_secret();
    for veil in [hidden, hidden.set(false)] {
        assert_eq!(veil.input_range(0..4, None), None);
        assert_eq!(veil.input_range(3..4, None), None);
        assert_eq!(veil.input_range(3..5, Some(3..5)), Some(3..5));
        assert_eq!(veil.input_range(4..5, Some(3..5)), Some(4..5));
        assert_eq!(veil.input_range(2..5, Some(3..5)), None);
        assert_eq!(veil.input_range(3..6, Some(3..5)), None);
        assert_eq!(veil.input_range(0..9, Some(3..5)), None);
    }
    assert_eq!(Veil::default().input_range(0..4, None), Some(0..4));
    assert_eq!(Veil::default().input_range(0..9, Some(3..5)), Some(0..9));
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
    let shifted = Modifiers {
        platform: true,
        shift: true,
        ..Modifiers::default()
    };
    assert!(is_shortcut(&shifted), "Cmd+Shift+Z stays redo");
    assert!(
        !edit_shortcut(&shifted),
        "Cmd+Shift+C, X, V and A are not ours"
    );
    assert!(edit_shortcut(&Modifiers {
        platform: true,
        ..Modifiers::default()
    }));
    assert!(!edit_shortcut(&keys(false, false)));
    assert_eq!(shortcut_letter("C"), "c");
    assert_eq!(shortcut_letter("v"), "v");
    assert_eq!(shortcut_letter("enter"), "enter");
}
