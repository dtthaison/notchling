#![cfg(any(target_os = "macos", target_os = "linux"))]

use notchling_engine::reply::exec::Pty;
use notchling_engine::reply::live::LiveTerminal;
use notchling_engine::terminal::input_box;
use notchling_engine::terminal::{PtyCommand, Shade};
use std::time::Duration;

fn sh(script: &str) -> LiveTerminal {
    let mut command = PtyCommand::new("/bin/sh");
    command.arg("-c").arg(script).cwd("/").size(10, 60);
    LiveTerminal::spawn(&command).expect("sh starts")
}

#[test]
fn a_live_terminal_reports_which_cells_are_faint_and_which_one_is_the_cursor() {
    let mut pty = sh("printf '\\033[7mT\\033[27m\\033[2mry\\033[22m x'; sleep 5");
    assert!(pty.wait_quiet(Duration::from_millis(200), Duration::from_secs(20)));
    assert_eq!(pty.screen()[0], "Try x");
    let shades = pty.shades();
    assert_eq!(
        shades[0][..5],
        [
            Shade::Cursor,
            Shade::Faint,
            Shade::Faint,
            Shade::Plain,
            Shade::Plain
        ]
    );
}

#[test]
fn a_hint_drawn_through_a_real_pty_reads_as_an_empty_box() {
    let rule = "─".repeat(40);
    let script = format!(
        "printf '\\033[?25l{rule}\\n\\342\\235\\257\\302\\240\\033[7mT\\033[27m\\033[2mry \"fix parser.rs\"\\033[22m\\n{rule}'; sleep 5"
    );
    let mut pty = sh(&script);
    assert!(pty.wait_quiet(Duration::from_millis(200), Duration::from_secs(20)));
    let lines = pty.screen();
    let (cursor, shades) = (pty.cursor(), pty.shades());
    assert_eq!(cursor, None);
    let input = input_box::read_shaded(&lines, cursor, &shades).expect("a box");
    assert!(input.empty, "{lines:?}");
    assert!(input_box::hint_shown(&lines, &shades));
    assert!(!input_box::read(&lines, cursor).expect("a box").empty);
}
