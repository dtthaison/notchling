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
    use notchling_engine::terminal::{StartStep, Terminal};
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

    /// The child reports a failed exec through a close-on-exec pipe, so the caller learns it at spawn
    /// instead of seeing a pty whose child exited 127.
    #[test]
    fn a_missing_program_is_an_error_at_spawn() {
        let scratch = Scratch::new("pty-missing");
        let mut command = PtyCommand::new(scratch.path("no-such-program"));
        command.cwd(&scratch.dir);
        let error = Pty::spawn(&command).expect_err("nothing to run");
        assert_eq!(errno(&error), Some(libc::ENOENT), "{error}");
        assert!(
            matches!(
                error,
                PtyError::Start {
                    step: StartStep::Program,
                    ..
                }
            ),
            "{error}"
        );
        assert!(error.to_string().contains("run the program"), "{error}");
    }

    #[test]
    fn a_missing_folder_is_an_error_at_spawn() {
        let scratch = Scratch::new("pty-nocwd");
        let mut command = PtyCommand::new("/bin/sh");
        command
            .arg("-c")
            .arg("true")
            .cwd(scratch.path("no/such/folder"));
        let error = Pty::spawn(&command).expect_err("nowhere to start");
        assert_eq!(errno(&error), Some(libc::ENOENT), "{error}");
        assert!(
            matches!(
                error,
                PtyError::Start {
                    step: StartStep::Folder,
                    ..
                }
            ),
            "{error}"
        );
        assert!(error.to_string().contains("enter the folder"), "{error}");
    }

    /// A program that starts and then exits 127 itself is not a spawn failure.
    #[test]
    fn a_program_that_runs_and_exits_127_still_spawns() {
        let scratch = Scratch::new("pty-127");
        let pty = Pty::spawn(&sh("exit 127", &scratch)).expect("it ran");
        assert_eq!(pty.wait().expect("waited"), 127);
    }

    /// The os error behind a spawn failure.
    fn errno(error: &PtyError) -> Option<i32> {
        match error {
            PtyError::Spawn(e) | PtyError::Io(e) | PtyError::Start { error: e, .. } => {
                e.raw_os_error()
            }
            _ => None,
        }
    }

    /// Lists, from inside a child, every descriptor above stderr that is a terminal (a pty master
    /// or slave) by walking /dev/fd.
    fn leaked_ttys() -> Vec<String> {
        let script = r#"for f in /dev/fd/*; do n=${f##*/}; if [ "$n" -gt 2 ] && [ -t "$n" ]; then echo "$n"; fi; done"#;
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the probe runs");
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .map(String::from)
            .collect()
    }

    /// With forkpty, the master was inheritable from the moment it opened until the parent set
    /// close-on-exec, so a `Command` another thread started in that window got it (and kept the pty
    /// open after the session ended). posix_openpt with O_CLOEXEC has no such window.
    #[test]
    fn a_command_started_meanwhile_inherits_no_pty_descriptor() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Instant;

        // The probe has to see a descriptor that does leak, or finding none proves nothing.
        // SAFETY: opens a pty master on purpose without close-on-exec, and closes it below.
        let leaking = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        assert!(leaking >= 0, "a pty to leak");
        let seen = leaked_ttys();
        // SAFETY: the descriptor opened above.
        unsafe { libc::close(leaking) };
        assert!(
            seen.contains(&leaking.to_string()),
            "the probe can't see a leaked pty ({leaking}): {seen:?}"
        );

        let scratch = Scratch::new("pty-leak");
        let stop = Arc::new(AtomicBool::new(false));
        let spawners: Vec<_> = (0..3)
            .map(|_| {
                let stop = stop.clone();
                let command = sh("exit 0", &scratch);
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        if let Ok(pty) = Pty::spawn(&command) {
                            let _ = pty.wait();
                        }
                    }
                })
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut leaked = Vec::new();
        while leaked.is_empty() && Instant::now() < deadline {
            leaked = leaked_ttys();
        }
        stop.store(true, Ordering::SeqCst);
        for spawner in spawners {
            spawner.join().expect("spawner thread");
        }
        assert!(leaked.is_empty(), "a Command inherited pty fds {leaked:?}");
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
