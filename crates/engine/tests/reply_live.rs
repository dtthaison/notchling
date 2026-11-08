//! The reply executor's terminal on E3a's real pty, with `/bin/sh` standing in for `claude`: the
//! [`Pty`] adapter, and a [`PtyScript`] (Trust Folder's shape) played into it. Nothing else is
//! started, and every child is ended when its terminal drops.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use notchling_engine::control::{Command, KeyStep, PtyScript, Quiet};
use notchling_engine::reply::exec::Pty;
use notchling_engine::reply::live::{play_script, script_command, LiveTerminal};
use notchling_engine::terminal::PtyCommand;
use std::path::Path;
use std::time::{Duration, Instant};

fn sh(script: &str) -> LiveTerminal {
    let mut command = PtyCommand::new("/bin/sh");
    command.arg("-c").arg(script).cwd("/").size(10, 60);
    LiveTerminal::spawn(&command).expect("sh starts")
}

fn shows(pty: &mut LiveTerminal, needle: &str) -> bool {
    pty.screen().iter().any(|l| l.contains(needle))
}

#[test]
fn types_into_the_pty_and_reads_its_screen() {
    let mut pty = sh("printf 'ready\\n'; IFS= read -r line; printf 'got:%s\\n' \"$line\"; sleep 5");
    assert!(pty.wait_quiet(Duration::from_millis(200), Duration::from_secs(5)));
    assert!(shows(&mut pty, "ready"), "{:?}", pty.screen());
    let before = pty.now_ms();
    pty.write(b"hi\r").expect("writes");
    pty.sleep(Duration::from_millis(400));
    assert!(pty.now_ms() >= before + 400);
    assert!(shows(&mut pty, "got:hi"), "{:?}", pty.screen());
}

#[test]
fn waiting_on_a_silent_program_says_nothing_came() {
    let mut pty = sh("sleep 5");
    let started = Instant::now();
    assert!(!pty.wait_quiet(Duration::from_millis(100), Duration::from_millis(300)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

fn script(program: &str, wait_timeout_secs: u64) -> PtyScript {
    let step = |keys: &str, pause_ms: u64, then_quiet: Option<Quiet>| KeyStep {
        keys: keys.to_string(),
        pause_ms,
        then_quiet,
    };
    PtyScript {
        command: Command::new(&["/bin/sh", "-c", program]).in_dir("/"),
        rows: 10,
        cols: 60,
        wait_for: "Yes, I trust this folder".into(),
        wait_timeout_secs,
        keys: vec![
            step("\u{1b}[B", 100, None),
            step(
                "\r",
                0,
                Some(Quiet {
                    quiet_ms: 200,
                    timeout_ms: 2000,
                }),
            ),
            step("\u{03}", 100, None),
        ],
    }
}

#[test]
fn a_script_waits_for_its_prompt_then_plays_its_keys() {
    let script = script(
        "sleep 0.3; printf 'Yes, I trust this folder\\n'; IFS= read -r a; printf 'after:%s\\n' \"$a\" | od -c | head -1; sleep 5",
        5,
    );
    let mut pty = LiveTerminal::spawn(&script_command(&script, Path::new("/unused/claude")))
        .expect("sh starts");
    assert!(play_script(&script, &mut pty));
    // Down then Enter reached the program: `read` got the arrow's bytes.
    let text = pty.screen().join("\n");
    assert!(text.contains("033   [   B"), "{text}");
}

#[test]
fn a_script_whose_prompt_never_shows_types_nothing() {
    let script = script(
        "printf 'something else\\n'; IFS= read -r a; printf 'typed\\n'; sleep 5",
        1,
    );
    let mut pty = LiveTerminal::spawn(&script_command(&script, Path::new("/unused/claude")))
        .expect("sh starts");
    let started = Instant::now();
    assert!(!play_script(&script, &mut pty));
    assert!(started.elapsed() < Duration::from_secs(4));
    pty.sleep(Duration::from_millis(300));
    assert!(!shows(&mut pty, "typed"), "{:?}", pty.screen());
}
