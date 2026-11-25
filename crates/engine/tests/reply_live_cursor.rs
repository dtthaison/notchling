#![cfg(any(target_os = "macos", target_os = "linux"))]

use notchling_engine::reply::exec::Pty;
use notchling_engine::reply::live::LiveTerminal;
use notchling_engine::terminal::PtyCommand;
use std::time::Duration;

fn sh(script: &str) -> LiveTerminal {
    let mut command = PtyCommand::new("/bin/sh");
    command.arg("-c").arg(script).cwd("/").size(10, 60);
    LiveTerminal::spawn(&command).expect("sh starts")
}

#[test]
fn the_cursor_is_reported_only_while_the_program_shows_it() {
    let mut pty = sh("printf 'ab\\033[?25l'; IFS= read -r go; printf '\\033[?25hc'; sleep 5");
    assert!(pty.wait_quiet(Duration::from_millis(200), Duration::from_secs(20)));
    assert_eq!(pty.screen()[0], "ab");
    assert_eq!(pty.cursor(), None, "hidden in the middle of a draw");
    pty.write(b"\r").expect("writes");
    let deadline = pty.now_ms() + 20_000;
    while pty.cursor().is_none() && pty.now_ms() < deadline {
        pty.sleep(Duration::from_millis(50));
    }
    assert_eq!(pty.screen()[1], "c");
    assert_eq!(pty.cursor(), Some((1, 1)));
}
