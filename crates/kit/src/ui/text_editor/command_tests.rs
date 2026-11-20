use super::*;

#[test]
fn copy_and_cut_need_a_selection_and_the_rest_do_not() {
    assert!(!takes(Command::Copy, true));
    assert!(!takes(Command::Cut, true));
    assert!(takes(Command::Copy, false));
    assert!(takes(Command::Cut, false));
    for command in [
        Command::Paste,
        Command::SelectAll,
        Command::Undo,
        Command::Redo,
    ] {
        assert!(takes(command, true));
        assert!(takes(command, false));
    }
}
