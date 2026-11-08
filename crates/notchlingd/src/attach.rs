//! The daemon's real terminal and launcher: [`ClaudeAttacher`] opens `claude attach <job>` on E3a's
//! pty for the reply executor, [`LiveLauncher`] is E11's [`RealLauncher`] with the hidden-pty
//! scripts (Trust Folder) played on the same pty. Both run only behind the acting switch, which is
//! off until the maintainer hands sessions over; tests use fakes.

use crate::acting_ops::Attacher;
use notchling_engine::control::{
    Command, LaunchError, LaunchOutput, Launcher, PtyScript, RealLauncher,
};
use notchling_engine::reply::exec::{Pty, ScreenCheck};
use notchling_engine::reply::live::{
    attach_command, play_script, script_command, AskDialog, LiveTerminal,
};
use notchling_local_api::{ApiError, ErrorCode};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `claude attach` for a background job.
pub struct ClaudeAttacher {
    claude: PathBuf,
}

impl ClaudeAttacher {
    pub fn new(claude: &Path) -> ClaudeAttacher {
        ClaudeAttacher {
            claude: claude.to_path_buf(),
        }
    }
}

impl Attacher for ClaudeAttacher {
    fn attach(&self, job: &str, config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError> {
        let terminal = LiveTerminal::spawn(&attach_command(&self.claude, job, config_dir))
            .map_err(|e| ApiError::new(ErrorCode::Failed, format!("couldn't attach: {e}")))?;
        Ok(Box::new(terminal))
    }

    fn screen_check(&self) -> &dyn ScreenCheck {
        &AskDialog
    }
}

/// [`RealLauncher`], plus a pty for the scripts it can't play.
pub struct LiveLauncher {
    real: RealLauncher,
}

impl LiveLauncher {
    pub fn new(real: RealLauncher) -> LiveLauncher {
        LiveLauncher { real }
    }
}

impl Launcher for LiveLauncher {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        self.real.run(command, timeout)
    }

    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError> {
        let mut terminal = LiveTerminal::spawn(&script_command(script, self.real.claude()))
            .map_err(|e| LaunchError::Spawn(e.to_string()))?;
        Ok(play_script(script, &mut terminal))
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use notchling_engine::control::{KeyStep, Quiet};

    /// A `claude` that is a shell script in a scratch folder: what the attach starts, with which
    /// arguments and lane, without any real Claude Code.
    fn fake_claude(tag: &str, body: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nl-attach-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let claude = dir.join("claude");
        std::fs::write(&claude, format!("#!/bin/sh\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, claude)
    }

    #[test]
    fn attaches_with_the_job_and_its_lane() {
        let (dir, claude) = fake_claude(
            "job",
            "printf 'args:%s %s lane:%s nc:%s\\n' \"$1\" \"$2\" \"$CLAUDE_CONFIG_DIR\" \"${NO_COLOR-unset}\"; sleep 5",
        );
        let attacher = ClaudeAttacher::new(&claude);
        let mut pty = attacher.attach("abcd1234", Some("/lanes/b")).unwrap();
        assert!(pty.wait_quiet(Duration::from_millis(200), Duration::from_secs(5)));
        let text = pty.screen().join("\n");
        assert!(
            text.contains("args:attach abcd1234 lane:/lanes/b nc:unset"),
            "{text}"
        );
        drop(pty);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_claude_fails_to_attach() {
        let attacher = ClaudeAttacher::new(Path::new("/nonexistent/claude"));
        match attacher.attach("abcd1234", None) {
            Err(e) => assert_eq!(e.code, ErrorCode::Failed),
            Ok(mut pty) => {
                // forkpty starts the child before exec fails: nothing is drawn and it ends.
                assert!(!pty.screen().iter().any(|l| l.contains("Yes, I trust")));
            }
        }
    }

    #[test]
    fn the_trust_script_runs_on_a_pty() {
        let (dir, claude) = fake_claude(
            "trust",
            "printf 'Yes, I trust this folder\\n'; IFS= read -r a; printf 'ok\\n' > \"$PWD/answered\"; sleep 5",
        );
        let launcher = LiveLauncher::new(RealLauncher::with_claude(claude));
        let script = PtyScript {
            command: Command::new(&["claude"]).in_dir(dir.to_str().unwrap()),
            rows: 10,
            cols: 60,
            wait_for: "Yes, I trust this folder".into(),
            wait_timeout_secs: 5,
            keys: vec![KeyStep {
                keys: "\r".into(),
                pause_ms: 0,
                then_quiet: Some(Quiet {
                    quiet_ms: 300,
                    timeout_ms: 2000,
                }),
            }],
        };
        assert_eq!(launcher.run_pty(&script), Ok(true));
        assert!(dir.join("answered").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
