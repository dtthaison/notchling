use super::exec::{Pty, ScreenCheck};
use super::keys::timing;
use crate::control::{PtyScript, CONFIG_DIR_VAR};
use crate::terminal::{ask_dialog, PtyCommand, PtyError, Terminal};
use std::path::Path;
use std::time::{Duration, Instant};

const DRAIN: Duration = Duration::from_millis(20);
const PROMPT_QUIET: Duration = Duration::from_millis(500);
const PROMPT_ROUND: Duration = Duration::from_secs(2);

pub struct LiveTerminal {
    terminal: Option<Terminal>,
    started: Instant,
}

impl LiveTerminal {
    pub fn spawn(command: &PtyCommand) -> Result<LiveTerminal, PtyError> {
        Ok(LiveTerminal {
            terminal: Some(Terminal::spawn(command)?),
            started: Instant::now(),
        })
    }
}

impl Drop for LiveTerminal {
    fn drop(&mut self) {
        if let Some(terminal) = self.terminal.take() {
            terminal.close();
        }
    }
}

impl Pty for LiveTerminal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match &self.terminal {
            Some(t) => t
                .write(bytes)
                .map_err(|e| std::io::Error::other(e.to_string())),
            None => Err(std::io::Error::other("the terminal is closed")),
        }
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        match self.terminal.as_mut() {
            Some(t) => t.wait_for_quiet(quiet, timeout).unwrap_or(false),
            None => false,
        }
    }

    fn screen(&mut self) -> Vec<String> {
        match self.terminal.as_mut() {
            Some(t) => {
                if !t.is_closed() {
                    let _ = t.wait_for_quiet(Duration::ZERO, DRAIN);
                }
                t.screen().lines()
            }
            None => Vec::new(),
        }
    }

    fn sleep(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        if let Some(t) = self.terminal.as_mut() {
            if !t.is_closed() {
                let _ = t.wait_for_quiet(duration, duration);
            }
        }
        let left = end.saturating_duration_since(Instant::now());
        if !left.is_zero() {
            std::thread::sleep(left);
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AskDialog;

impl ScreenCheck for AskDialog {
    fn shows(&self, question: &str, lines: &[String]) -> bool {
        ask_dialog::shows(question, lines)
    }
    fn is_review(&self, lines: &[String]) -> bool {
        ask_dialog::is_review(lines)
    }
}

fn config_dir(command: &mut PtyCommand, dir: Option<&str>) {
    match dir {
        Some(dir) => command.env(CONFIG_DIR_VAR, dir),
        None => command.env_remove(CONFIG_DIR_VAR),
    };
}

pub fn attach_command(claude: &Path, job: &str, dir: Option<&str>) -> PtyCommand {
    let mut command = PtyCommand::new(claude);
    command
        .arg("attach")
        .arg(job)
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .size(timing::ROWS, timing::COLS);
    config_dir(&mut command, dir);
    command
}

pub fn script_command(script: &PtyScript, claude: &Path) -> PtyCommand {
    let argv = &script.command.argv;
    let program = match argv.first().map(String::as_str) {
        Some("claude") | None => claude.to_path_buf(),
        Some(other) => Path::new(other).to_path_buf(),
    };
    let mut command = PtyCommand::new(program);
    command
        .args(argv.iter().skip(1))
        .size(script.rows, script.cols);
    if let Some(cwd) = &script.command.cwd {
        command.cwd(cwd);
    }
    for (key, value) in &script.command.env {
        match value {
            Some(v) => command.env(key, v),
            None => command.env_remove(key),
        };
    }
    command.env_remove("NO_COLOR");
    command
}

pub fn play_script(script: &PtyScript, pty: &mut dyn Pty) -> bool {
    let asking = |pty: &mut dyn Pty| pty.screen().iter().any(|l| l.contains(&script.wait_for));
    let deadline = pty
        .now_ms()
        .saturating_add(script.wait_timeout_secs.saturating_mul(1000));
    while !asking(pty) {
        if pty.now_ms() >= deadline {
            return false;
        }
        if !pty.wait_quiet(PROMPT_QUIET, PROMPT_ROUND) {
            pty.sleep(Duration::from_millis(100));
        }
    }
    for step in &script.keys {
        if pty.write(step.keys.as_bytes()).is_err() {
            return false;
        }
        if step.pause_ms > 0 {
            pty.sleep(Duration::from_millis(step.pause_ms));
        }
        if let Some(q) = step.then_quiet {
            pty.wait_quiet(
                Duration::from_millis(q.quiet_ms),
                Duration::from_millis(q.timeout_ms),
            );
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::plan_trust_folder;
    use std::ffi::OsString;

    fn env_of(command: &PtyCommand, key: &str) -> Option<String> {
        command
            .child_env()
            .find(|(k, _)| k.as_os_str() == key)
            .map(|(_, v)| v.to_string_lossy().into_owned())
    }

    fn args(command: &PtyCommand) -> Vec<OsString> {
        command.args.clone()
    }

    #[test]
    fn the_attach_command_is_reply_senders() {
        let lane = attach_command(Path::new("/c/claude"), "abcd1234", Some("/lanes/b"));
        assert_eq!(lane.program(), Path::new("/c/claude"));
        assert_eq!(args(&lane), ["attach", "abcd1234"]);
        assert_eq!((lane.rows(), lane.cols()), (50, 200));
        assert_eq!(env_of(&lane, "TERM").as_deref(), Some("xterm-256color"));
        assert_eq!(
            env_of(&lane, "CLAUDE_CONFIG_DIR").as_deref(),
            Some("/lanes/b")
        );
        assert_eq!(env_of(&lane, "NO_COLOR"), None);
        let main = attach_command(Path::new("/c/claude"), "abcd1234", None);
        assert_eq!(env_of(&main, "CLAUDE_CONFIG_DIR"), None);
    }

    #[test]
    fn the_trust_script_runs_claude_in_the_folder_for_its_lane() {
        let script = plan_trust_folder("/work/new", Some("/lanes/a"));
        let command = script_command(&script, Path::new("/c/claude"));
        assert_eq!(command.program(), Path::new("/c/claude"));
        assert!(args(&command).is_empty());
        assert_eq!(command.cwd.as_deref(), Some(Path::new("/work/new")));
        assert_eq!((command.rows(), command.cols()), (50, 200));
        assert_eq!(
            env_of(&command, "CLAUDE_CONFIG_DIR").as_deref(),
            Some("/lanes/a")
        );
        assert_eq!(env_of(&command, "TERM").as_deref(), Some("xterm-256color"));
        assert_eq!(env_of(&command, "NO_COLOR"), None);
    }

    struct Scripted {
        screens: Vec<Vec<String>>,
        reads: usize,
        writes: Vec<(u64, String)>,
        clock: u64,
    }

    impl Pty for Scripted {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.writes
                .push((self.clock, String::from_utf8_lossy(bytes).into_owned()));
            Ok(())
        }
        fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
            self.clock += quiet.as_millis() as u64;
            true
        }
        fn screen(&mut self) -> Vec<String> {
            let i = self.reads.min(self.screens.len() - 1);
            self.reads += 1;
            self.screens[i].clone()
        }
        fn sleep(&mut self, d: Duration) {
            self.clock += d.as_millis() as u64;
        }
        fn now_ms(&self) -> u64 {
            self.clock
        }
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn trust_waits_for_the_prompt_then_types_down_enter_and_two_ctrl_c() {
        let script = plan_trust_folder("/w", None);
        let mut pty = Scripted {
            screens: vec![
                lines(&["Welcome"]),
                lines(&["Welcome"]),
                lines(&["❯ 1. No, exit", "  2. Yes, I trust this folder"]),
            ],
            reads: 0,
            writes: Vec::new(),
            clock: 0,
        };
        assert!(play_script(&script, &mut pty));
        let keys: Vec<&str> = pty.writes.iter().map(|(_, k)| k.as_str()).collect();
        assert_eq!(keys, ["\u{1b}[B", "\r", "\u{03}", "\u{03}"]);
        assert_eq!(pty.writes[1].0 - pty.writes[0].0, 300);
    }

    #[test]
    fn no_prompt_by_the_deadline_types_nothing() {
        let script = plan_trust_folder("/w", None);
        let mut pty = Scripted {
            screens: vec![lines(&["Something else"])],
            reads: 0,
            writes: Vec::new(),
            clock: 0,
        };
        assert!(!play_script(&script, &mut pty));
        assert!(pty.writes.is_empty());
        assert!(pty.clock >= 20_000, "waited the script's 20 s");
    }
}
