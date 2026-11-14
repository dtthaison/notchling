mod common;

use common::notchling_fixture;
use notchling_engine::reply::keys;
use notchling_engine::terminal::input_box::{self, Input};
use notchling_engine::terminal::TerminalScreen;
use std::path::Path;

fn shot(name: &str) -> (Vec<String>, Option<(usize, usize)>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/input-box")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
    assert_eq!(head[0], "cursor", "{name}");
    let cursor = (head[1].parse().unwrap(), head[2].parse().unwrap());
    let shown = head[3] == "true";
    (lines.map(String::from).collect(), shown.then_some(cursor))
}

fn read(name: &str) -> Input {
    let (lines, cursor) = shot(name);
    assert!(!keys::dialog_open(&lines), "{name}");
    input_box::read(&lines, cursor).unwrap_or_else(|| panic!("{name}: no input box"))
}

fn recorded(name: &str) -> TerminalScreen {
    let path = notchling_fixture(&format!("terminal/{name}"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(&bytes);
    screen
}

#[test]
fn a_new_jobs_hint_is_an_empty_box() {
    let input = read("new-job-hint.txt");
    assert_eq!(input.held, ["❯\u{a0}Try \"how do I log an error?\""]);
    assert!(input.empty);
    let (lines, _) = shot("new-job-hint.txt");
    assert!(
        !input_box::read(&lines, None).unwrap().empty,
        "without the cursor the hint reads as typed text, which only delays a reply"
    );
}

#[test]
fn text_and_pictures_left_in_the_box_are_not_empty() {
    let typed = read("typed.txt");
    assert_eq!(typed.held, ["❯\u{a0}Reply with just the word ok."]);
    assert!(!typed.empty);
    let left = read("pictures-left-unsent.txt");
    assert_eq!(
        left.held,
        ["❯\u{a0}[Image #5] [Image #6]Say only the word race."]
    );
    assert!(!left.empty);
}

#[test]
fn a_sent_message_leaves_the_box_empty_while_the_session_works() {
    let input = read("working-after-send.txt");
    assert_eq!(input.held, ["❯"]);
    assert!(input.empty);
}

#[test]
fn a_queued_messages_hint_is_an_empty_box() {
    let input = read("queued-hint.txt");
    assert_eq!(input.held, ["❯\u{a0}Press up to edit queued messages"]);
    assert!(input.empty);
}

#[test]
fn shell_mode_many_lines_a_folded_paste_and_the_command_menu_all_hold_text() {
    let shell = read("shell-mode.txt");
    assert_eq!(shell.held, ["!\u{a0}ls -la"]);
    assert!(!shell.empty);
    let lines = read("three-lines.txt");
    assert_eq!(lines.held, ["❯\u{a0}one", "  two", "  three"]);
    assert!(!lines.empty);
    let folded = read("long-paste.txt");
    assert_eq!(folded.held, ["❯\u{a0}[Pasted text #7 +39 lines]"]);
    assert!(!folded.empty);
    let menu = read("slash-menu.txt");
    assert_eq!(menu.held, ["❯\u{a0}/compact"]);
    assert!(!menu.empty);
}

#[test]
fn every_recorded_prompt_reads_as_an_empty_box() {
    for name in [
        "attach-idle.bin",
        "reply-paced.bin",
        "clear-input-draft.bin",
        "paste-multiline-clear.bin",
        "plugin-0-attach.bin",
        "queued-while-working.bin",
        "take-back-queued.bin",
        "esc-working.bin",
    ] {
        let screen = recorded(name);
        let lines = screen.lines();
        assert!(!keys::dialog_open(&lines), "{name}");
        let input = input_box::read(&lines, Some(screen.cursor()))
            .unwrap_or_else(|| panic!("{name}: no input box"));
        assert_eq!(input.held, ["❯"], "{name}");
        assert!(input.empty, "{name}");
        assert!(input_box::read(&lines, None).unwrap().empty, "{name}");
    }
}

#[test]
fn a_question_on_screen_is_refused_before_its_rules_are_read_as_a_box() {
    let lines = recorded("question-open.bin").lines();
    assert!(keys::dialog_open(&lines));
}

#[test]
fn a_screen_of_plugins_has_no_input_box() {
    let lines = recorded("plugin-2-enter.bin").lines();
    assert_eq!(input_box::read(&lines, None), None);
}

#[test]
fn the_transcript_view_has_no_input_box_and_is_no_dialog() {
    let (lines, cursor) = shot("transcript-view.txt");
    assert_eq!(cursor, None, "its cursor is hidden");
    assert!(!keys::dialog_open(&lines));
    assert_eq!(input_box::read(&lines, cursor), None);
}
