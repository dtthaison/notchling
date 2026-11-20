use notchling_engine::terminal::screen_dialog;
use notchling_engine::terminal::TerminalScreen;
use std::path::Path;

fn screen_of(text: &str) -> TerminalScreen {
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(text.replace('\n', "\r\n").as_bytes());
    screen
}

fn made_up(name: &str) -> TerminalScreen {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dialog-made-up")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    screen_of(&text)
}

fn read(name: &str) -> Option<screen_dialog::ScreenDialog> {
    screen_dialog::read(&made_up(name).lines())
}

#[test]
fn a_numbered_list_in_normal_output_is_no_dialog() {
    assert_eq!(read("list-in-output.txt"), None);
}

#[test]
fn a_list_still_streaming_is_no_dialog() {
    assert_eq!(read("list-streaming.txt"), None);
}

#[test]
fn a_selected_list_with_a_spinner_below_is_no_dialog() {
    assert_eq!(read("list-selected-then-spinner.txt"), None);
}

#[test]
fn a_permission_prompt_is_not_a_screen_dialog() {
    assert_eq!(read("permission-prompt.txt"), None);
}

#[test]
fn two_lists_across_a_rule_are_not_one_dialog() {
    assert_eq!(read("two-lists-across-a-rule.txt"), None);
}

#[test]
fn an_option_numbered_zero_under_a_one_reads_as_none_and_does_not_panic() {
    let screen = screen_of(" Pick\n ❯ 1. First\n   0. Zero\n\n Enter to confirm · Esc to cancel\n");
    assert_eq!(screen_dialog::read(&screen.lines()), None);
}

#[test]
fn a_hint_row_after_the_last_option_is_not_part_of_its_label() {
    let screen = screen_of(
        "────────────────────────────────────────────────────────────────────────────────\n Switch model?\n\n ❯ 1. Yes, switch\n   2. No, go back\n ↑/↓ to navigate\n",
    );
    let dialog = screen_dialog::read(&screen.lines()).expect("a dialog");
    assert_eq!(dialog.options[1].label, "No, go back");
}

#[test]
fn a_prompt_box_below_the_options_means_the_dialog_is_over() {
    let screen = screen_of(
        " Switch model?\n\n ❯ 1. Yes, switch\n   2. No, go back\n\n────────────────────────────────────────────────────────────────────────────────\n❯\n────────────────────────────────────────────────────────────────────────────────\n  ? for shortcuts\n",
    );
    assert_eq!(screen_dialog::read(&screen.lines()), None);
}
