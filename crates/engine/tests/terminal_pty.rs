//! The pty: `/bin/cat` and `/bin/sh` on a forkpty, in a scratch folder. Nothing else is started, and
//! every child is ended when its `Terminal` drops.

mod common;

use common::Scratch;
use notchling_engine::terminal::{Pty, PtyCommand, PtyError};

fn sh(script: &str, scratch: &Scratch) -> PtyCommand {
    let mut command = PtyCommand::new("/bin/sh");
    command.arg("-c").arg(script).cwd(&scratch.dir).size(10, 60);
    command
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix {
    use super::*;
    use notchling_engine::terminal::Terminal;
    use std::time::Duration;

    fn text(terminal: &Terminal) -> String {
        terminal.screen().lines().join("\n")
    }

    /// Reads until `needle` is on screen, or fails with what is there.
    fn wait_for(terminal: &mut Terminal, needle: &str) {
        let found = terminal
            .wait_until(Duration::from_secs(5), |s| {
                s.lines().join("\n").contains(needle)
            })
            .expect("reading the pty");
        assert!(found, "{needle:?} never showed:\n{}", text(terminal));
    }

    #[test]
    fn cat_echoes_what_is_typed() {
        let scratch = Scratch::new("pty-cat");
        let mut command = PtyCommand::new("/bin/cat");
        command.cwd(&scratch.dir).size(10, 40);
        let mut terminal = Terminal::spawn(&command).expect("cat starts");
        terminal.write(b"hello pty\n").expect("typed");
        // The tty echoes the line, then cat prints it back.
        let twice = terminal
            .wait_until(Duration::from_secs(5), |s| {
                s.lines()
                    .iter()
                    .filter(|l| l.as_str() == "hello pty")
                    .count()
                    == 2
            })
            .expect("reading");
        assert!(twice, "{}", text(&terminal));
    }

    #[test]
    fn the_child_starts_at_the_size_asked_for() {
        let scratch = Scratch::new("pty-size");
        let mut command = sh("stty size", &scratch);
        command.size(12, 34);
        let mut terminal = Terminal::spawn(&command).expect("sh starts");
        wait_for(&mut terminal, "12 34");
        assert_eq!(
            (terminal.screen().rows(), terminal.screen().cols()),
            (12, 34)
        );
    }

    /// The pty is the child's controlling terminal, so a resize reaches it as SIGWINCH (with
    /// posix_spawn, `claude attach` got the signal but never repainted).
    #[test]
    fn a_resize_reaches_the_child() {
        let scratch = Scratch::new("pty-winch");
        let command = sh(
            "trap 'stty size' WINCH; echo ready; while :; do sleep 0.05; done",
            &scratch,
        );
        let mut terminal = Terminal::spawn(&command).expect("sh starts");
        wait_for(&mut terminal, "ready");
        terminal.resize(33, 77).expect("resized");
        assert_eq!(
            (terminal.screen().rows(), terminal.screen().cols()),
            (33, 77)
        );
        wait_for(&mut terminal, "33 77");
    }

    #[test]
    fn the_pty_is_the_controlling_terminal() {
        let scratch = Scratch::new("pty-tty");
        let mut terminal = Terminal::spawn(&sh("tty; echo done", &scratch)).expect("sh starts");
        wait_for(&mut terminal, "done");
        assert!(text(&terminal).contains("/dev/"), "{}", text(&terminal));
        assert!(!text(&terminal).contains("not a tty"));
    }

    /// An inherited NO_COLOR left Claude Code's menus all one gray.
    #[test]
    fn no_color_is_dropped() {
        let scratch = Scratch::new("pty-nocolor");
        let mut command = sh(
            "echo \"nc=${NO_COLOR-unset} term=$TERM other=$KEEP\"",
            &scratch,
        );
        command.env("NO_COLOR", "1").env("KEEP", "yes");
        let mut terminal = Terminal::spawn(&command).expect("sh starts");
        wait_for(&mut terminal, "nc=unset term=xterm-256color other=yes");
    }

    /// Answered, Claude Code dropped its colors and the answer landed in its input box. With cat
    /// reading, an answer would be echoed onto the screen.
    #[test]
    fn device_attribute_queries_get_no_answer() {
        let scratch = Scratch::new("pty-da");
        let command = sh(
            "printf '\\033[c\\033[>c\\033[>0q'; echo ready; exec /bin/cat",
            &scratch,
        );
        let mut terminal = Terminal::spawn(&command).expect("sh starts");
        wait_for(&mut terminal, "ready");
        terminal
            .wait_for_quiet(Duration::from_millis(400), Duration::from_secs(1))
            .expect("reading");
        let drawn: Vec<String> = terminal
            .screen()
            .lines()
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(drawn, ["ready"], "nothing was written back");
    }

    #[test]
    fn it_runs_in_the_folder_given() {
        let scratch = Scratch::new("pty-cwd");
        let mut command = sh("pwd -P", &scratch);
        command.size(10, 400); // a temp path is longer than one 60-column line
        let mut terminal = Terminal::spawn(&command).expect("sh starts");
        let real = std::fs::canonicalize(&scratch.dir).expect("canonical");
        wait_for(&mut terminal, &real.to_string_lossy());
    }

    #[test]
    fn exit_status_and_terminate() {
        let scratch = Scratch::new("pty-exit");
        let pty = Pty::spawn(&sh("exit 3", &scratch)).expect("sh starts");
        assert_eq!(pty.wait().expect("waited"), 3);
        assert_eq!(
            pty.try_wait().expect("asked"),
            Some(3),
            "the status is kept"
        );

        let mut cat = PtyCommand::new("/bin/cat");
        cat.cwd(&scratch.dir);
        let pty = Pty::spawn(&cat).expect("cat starts");
        assert_eq!(pty.try_wait().expect("asked"), None, "cat waits for input");
        let status = pty.terminate(Duration::from_secs(2));
        assert_eq!(status, Some(128 + 15), "ended by SIGTERM");
    }

    #[test]
    fn a_program_that_cannot_run_exits_127() {
        let scratch = Scratch::new("pty-missing");
        let mut command = PtyCommand::new(scratch.path("no-such-program"));
        command.cwd(&scratch.dir);
        let pty = Pty::spawn(&command).expect("the fork itself works");
        assert_eq!(pty.wait().expect("waited"), 127);
    }

    #[test]
    fn reading_after_the_child_ends_says_closed() {
        let scratch = Scratch::new("pty-eof");
        let pty = Pty::spawn(&sh("echo bye", &scratch)).expect("sh starts");
        let mut buf = [0u8; 4096];
        let mut seen = Vec::new();
        loop {
            match pty
                .read_timeout(&mut buf, Duration::from_secs(5))
                .expect("read")
            {
                notchling_engine::terminal::ReadOutcome::Data(n) => {
                    seen.extend_from_slice(&buf[..n])
                }
                notchling_engine::terminal::ReadOutcome::Closed => break,
                notchling_engine::terminal::ReadOutcome::Timeout => panic!("no end of file"),
            }
        }
        assert!(String::from_utf8_lossy(&seen).contains("bye"));
    }

    #[test]
    fn a_nul_in_an_argument_is_refused() {
        let mut command = PtyCommand::new("/bin/sh");
        command.arg("a\0b");
        assert!(matches!(Pty::spawn(&command), Err(PtyError::Nul)));
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn elsewhere_a_pty_is_unsupported() {
    let scratch = Scratch::new("pty-unsupported");
    assert!(matches!(
        Pty::spawn(&sh("true", &scratch)),
        Err(PtyError::Unsupported)
    ));
}
