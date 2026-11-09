//! The permission dialog read off a whole attach screen. NOT live recordings: the screens in
//! `fixtures/permission-made-up/` were written from Claude Code's known layout (a full-width rule,
//! the tool's title, the command, "Do you want to proceed?", numbered options; older builds in a
//! box), since no job blocked on a permission has been recorded yet (docs/recording.md,
//! `dialog-permission`). The recorded AskUserQuestion and idle screens check that neither reads as
//! a permission.

mod common;

use common::notchling_fixture;
use notchling_engine::terminal::permission_dialog::{self, Decision, Expected};
use notchling_engine::terminal::{dialog_open, TerminalScreen};
use std::path::Path;

fn screen_of(bytes: &[u8]) -> TerminalScreen {
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(bytes);
    screen
}

/// A made-up screen, drawn by the emulator as `claude attach` output would be.
fn made_up(name: &str) -> TerminalScreen {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/permission-made-up")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    screen_of(text.replace('\n', "\r\n").as_bytes())
}

fn recorded(name: &str) -> TerminalScreen {
    let path = notchling_fixture(&format!("terminal/{name}"));
    screen_of(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

fn expected(tool: &str, summary: &str) -> Expected {
    Expected {
        tool: tool.into(),
        summary: summary.into(),
    }
}

#[test]
fn a_bash_dialog() {
    let screen = made_up("bash.txt");
    assert!(dialog_open(&screen), "the reply path refuses it too");
    let d = permission_dialog::read(&screen.lines()).expect("a dialog");
    assert_eq!(d.title, "Bash command");
    assert_eq!(d.question, "Do you want to proceed?");
    assert_eq!(
        d.body,
        [
            "rm -rf build && cargo build --release --workspace",
            "Remove the build folder and rebuild"
        ]
    );
    assert_eq!(d.options.len(), 3);
    assert_eq!(d.option_for(Decision::Allow).map(|o| o.number), Some(1));
    assert_eq!(d.option_for(Decision::Always).map(|o| o.number), Some(2));
    assert_eq!(d.option_for(Decision::Deny).map(|o| o.number), Some(3));
    assert!(d.asks(&expected(
        "Bash",
        "rm -rf build && cargo build --release --workspace"
    )));
    assert!(d.asks(&expected("Bash", "rm -rf build && cargo build …")));
    // The command higher up, in Claude's own tool line, isn't what's asked about.
    assert!(!d.asks(&expected("Bash", "rm -rf build && cargo test")));
}

#[test]
fn a_boxed_dialog() {
    let d = permission_dialog::read(&made_up("bash-boxed.txt").lines()).expect("a dialog");
    assert_eq!(d.title, "Bash command");
    assert_eq!(
        d.options[1].label,
        "Yes, and don't ask again for git push commands in /Users/me/work/api"
    );
    assert!(d.asks(&expected(
        "Bash",
        "git push --force-with-lease origin agent/42-fix"
    )));
}

#[test]
fn an_edit_has_no_always_option() {
    let d = permission_dialog::read(&made_up("edit.txt").lines()).expect("a dialog");
    assert_eq!(d.title, "Edit file");
    assert_eq!(d.question, "Do you want to make this edit to main.rs?");
    assert_eq!(d.option_for(Decision::Allow).map(|o| o.number), Some(1));
    assert_eq!(d.option_for(Decision::Always), None);
    assert_eq!(d.option_for(Decision::Deny).map(|o| o.number), Some(3));
    // The job's state names the whole path; the dialog its file.
    assert!(d.asks(&expected("Edit", "/Users/me/work/api/src/main.rs")));
    assert!(!d.asks(&expected("Edit", "/Users/me/work/api/src/lib.rs")));
    assert!(!d.asks(&expected("Bash", "src/main.rs")));
}

#[test]
fn a_wrapped_command_matches_the_one_line_summary() {
    let d = permission_dialog::read(&made_up("bash-wrapped.txt").lines()).expect("a dialog");
    let command = format!("echo {}{} && ls", "x".repeat(190), "y".repeat(40));
    assert!(d.asks(&expected("Bash", &command)));
    assert_eq!(d.option_for(Decision::Always), None);
    assert_eq!(d.option_for(Decision::Deny).map(|o| o.number), Some(2));
}

#[test]
fn recorded_screens_without_a_permission_read_as_none() {
    for name in [
        "question-open.bin",
        "attach-idle.bin",
        "plugin-0-attach.bin",
    ] {
        assert_eq!(
            permission_dialog::read(&recorded(name).lines()),
            None,
            "{name}"
        );
    }
}
