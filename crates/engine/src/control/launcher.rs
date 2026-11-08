//! Starting processes. Everything that launches `claude` or `git` goes through [`Launcher`], so a
//! test swaps in [`FakeLauncher`] and no test ever runs a real session.

use super::plan::{Command, PtyScript};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What a finished process left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOutput {
    /// The exit code; -1 when a signal ended it.
    pub code: i32,
    /// Standard output.
    pub output: String,
    /// Standard error. `git` reads only [`output`](Self::output) (the Swift app sends its stderr to
    /// nowhere); `claude` messages use [`combined`](Self::combined), as `runCapturing` does.
    pub stderr: String,
}

impl LaunchOutput {
    pub fn ok(output: &str) -> Self {
        LaunchOutput {
            code: 0,
            output: output.to_string(),
            stderr: String::new(),
        }
    }

    pub fn failed(code: i32, output: &str) -> Self {
        LaunchOutput {
            code,
            output: output.to_string(),
            stderr: String::new(),
        }
    }

    /// Standard output, then standard error.
    pub fn combined(&self) -> String {
        format!("{}{}", self.output, self.stderr)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The process could not be started (no such program, no such folder).
    Spawn(String),
    /// It did not finish in this many seconds and was killed.
    Timeout(u64),
    /// This launcher cannot do that (a pty on a platform without one built).
    Unsupported,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Spawn(why) => write!(f, "couldn't start it: {why}"),
            LaunchError::Timeout(secs) => write!(f, "didn't finish in {secs} s"),
            LaunchError::Unsupported => f.write_str("not supported here"),
        }
    }
}

impl std::error::Error for LaunchError {}

pub trait Launcher: Send + Sync {
    /// Runs `command` to its end (or `timeout`), with its output collected.
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError>;

    /// Plays a key script into a hidden pty; true when the prompt it waits for appeared and the keys
    /// went in. The real pty belongs to `ptyhost`; [`RealLauncher`] answers `Unsupported`.
    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError>;
}

/// What a [`FakeLauncher`] was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Run(Command, Duration),
    Pty(PtyScript),
}

/// Records every call and answers from a queue (an empty queue answers exit 0 with no output).
#[derive(Default)]
pub struct FakeLauncher {
    calls: Mutex<Vec<Call>>,
    outputs: Mutex<std::collections::VecDeque<Result<LaunchOutput, LaunchError>>>,
    pty: Mutex<std::collections::VecDeque<Result<bool, LaunchError>>>,
}

impl FakeLauncher {
    pub fn new() -> Self {
        FakeLauncher::default()
    }

    /// The next `run` answers this.
    pub fn push_output(&self, answer: Result<LaunchOutput, LaunchError>) {
        lock(&self.outputs).push_back(answer);
    }

    /// The next `run_pty` answers this (the default is `Ok(true)`).
    pub fn push_pty(&self, answer: Result<bool, LaunchError>) {
        lock(&self.pty).push_back(answer);
    }

    pub fn calls(&self) -> Vec<Call> {
        lock(&self.calls).clone()
    }

    /// The commands of the `run` calls, in order.
    pub fn commands(&self) -> Vec<Command> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Run(command, _) => Some(command),
                Call::Pty(_) => None,
            })
            .collect()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Launcher for FakeLauncher {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        lock(&self.calls).push(Call::Run(command.clone(), timeout));
        lock(&self.outputs)
            .pop_front()
            .unwrap_or_else(|| Ok(LaunchOutput::ok("")))
    }

    fn run_pty(&self, script: &PtyScript) -> Result<bool, LaunchError> {
        lock(&self.calls).push(Call::Pty(script.clone()));
        lock(&self.pty).pop_front().unwrap_or(Ok(true))
    }
}

/// Starts real processes with `std::process`.
///
/// The environment is the daemon's own, less `NO_COLOR` (it left Claude Code's menus gray), then the
/// command's changes. Output goes to a temporary file rather than a pipe: the first `claude --bg` in
/// a lane starts a host that inherits the output, so reading a pipe to its end would wait as long as
/// that host lives. Input is always null.
pub struct RealLauncher {
    claude: PathBuf,
}

/// `claude` is `<home>/.local/bin/claude` when that is a file, else `/usr/local/bin/claude`. The
/// reply attach (`reply::live`) runs the same one.
pub fn claude_path(home: &Path) -> PathBuf {
    let local = home.join(".local").join("bin").join("claude");
    if local.is_file() {
        local
    } else {
        PathBuf::from("/usr/local/bin/claude")
    }
}

/// Variables never passed to a launched program.
const DROPPED_ENV: [&str; 1] = ["NO_COLOR"];

impl RealLauncher {
    /// `claude` is found by [`claude_path`].
    pub fn new(home: &Path) -> Self {
        RealLauncher {
            claude: claude_path(home),
        }
    }

    /// The `claude` this launcher runs.
    pub fn claude(&self) -> &Path {
        &self.claude
    }

    pub fn with_claude(path: PathBuf) -> Self {
        RealLauncher { claude: path }
    }

    fn program(&self, name: &str) -> PathBuf {
        match name {
            "claude" => self.claude.clone(),
            "git" if Path::new("/usr/bin/git").is_file() => PathBuf::from("/usr/bin/git"),
            other => PathBuf::from(other),
        }
    }
}

static OUT_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Launcher for RealLauncher {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        let Some(name) = command.argv.first() else {
            return Err(LaunchError::Spawn("empty command".to_string()));
        };
        let file = std::env::temp_dir().join(format!(
            "notchling-{}-{}.out",
            std::process::id(),
            OUT_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let result = self.run_to_file(name, command, timeout, &file);
        let _ = std::fs::remove_file(&file);
        result
    }

    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

impl RealLauncher {
    fn run_to_file(
        &self,
        name: &str,
        command: &Command,
        timeout: Duration,
        file: &Path,
    ) -> Result<LaunchOutput, LaunchError> {
        let err_file = file.with_extension("err");
        let out = std::fs::File::create(file).map_err(|e| LaunchError::Spawn(e.to_string()))?;
        let err =
            std::fs::File::create(&err_file).map_err(|e| LaunchError::Spawn(e.to_string()))?;
        let mut process = std::process::Command::new(self.program(name));
        process
            .args(&command.argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(out)
            .stderr(err);
        if let Some(cwd) = &command.cwd {
            process.current_dir(cwd);
        }
        for key in DROPPED_ENV {
            process.env_remove(key);
        }
        for (key, value) in &command.env {
            match value {
                Some(v) => process.env(key, v),
                None => process.env_remove(key),
            };
        }
        let mut child = process
            .spawn()
            .map_err(|e| LaunchError::Spawn(e.to_string()))?;
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(LaunchError::Timeout(timeout.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(LaunchError::Spawn(e.to_string())),
            }
        };
        let read = |path: &Path| {
            String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned()
        };
        let result = LaunchOutput {
            code: status.code().unwrap_or(-1),
            output: read(file),
            stderr: read(&err_file),
        };
        let _ = std::fs::remove_file(&err_file);
        Ok(result)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn real() -> RealLauncher {
        RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"))
    }

    #[test]
    fn runs_a_program_and_collects_output_and_code() {
        let command = Command::new(&["/bin/sh", "-c", "echo out; echo err 1>&2; exit 3"]);
        let got = real().run(&command, Duration::from_secs(10)).expect("runs");
        assert_eq!(got.code, 3);
        assert_eq!(got.output.trim(), "out");
        assert_eq!(got.stderr.trim(), "err");
        assert_eq!(got.combined().trim(), "out\nerr");
    }

    #[test]
    fn no_color_is_never_inherited_and_overrides_apply() {
        std::env::set_var("NO_COLOR", "1");
        std::env::set_var("NOTCHLING_TEST_NO_COLOR_PROBE", "x");
        let command = Command::new(&[
            "/bin/sh",
            "-c",
            "echo [${NO_COLOR-unset}][${CLAUDE_CONFIG_DIR-unset}][${NOTCHLING_TEST_NO_COLOR_PROBE-unset}]",
        ])
        .with_env("NOTCHLING_TEST_NO_COLOR_PROBE", None)
        .for_config_dir(Some("/lanes/a"));
        let got = real().run(&command, Duration::from_secs(10)).expect("runs");
        assert_eq!(got.output.trim(), "[unset][/lanes/a][unset]");
    }

    #[test]
    fn runs_in_the_planned_folder() {
        let command = Command::new(&["/bin/pwd"]).in_dir("/");
        let got = real().run(&command, Duration::from_secs(10)).expect("runs");
        assert_eq!(got.output.trim(), "/");
    }

    #[test]
    fn a_slow_process_is_killed_at_the_timeout() {
        let command = Command::new(&["/bin/sleep", "5"]);
        let got = real().run(&command, Duration::from_millis(200));
        assert_eq!(got, Err(LaunchError::Timeout(0)));
    }

    #[test]
    fn a_missing_program_is_a_spawn_error_and_a_pty_is_unsupported() {
        let got = real().run(&Command::new(&["/no/such/program"]), Duration::from_secs(1));
        assert!(matches!(got, Err(LaunchError::Spawn(_))));
        let script = super::super::trust::plan_trust_folder("/tmp", None);
        assert_eq!(real().run_pty(&script), Err(LaunchError::Unsupported));
    }
}

#[cfg(test)]
mod fake_tests {
    use super::*;

    #[test]
    fn the_fake_records_calls_and_replays_canned_output() {
        let fake = FakeLauncher::new();
        fake.push_output(Ok(LaunchOutput::failed(2, "boom")));
        let command = Command::new(&["claude", "stop", "ab12"]);
        let first = fake.run(&command, Duration::from_secs(60));
        assert_eq!(first, Ok(LaunchOutput::failed(2, "boom")));
        assert_eq!(
            fake.run(&command, Duration::from_secs(60)),
            Ok(LaunchOutput::ok(""))
        );
        assert_eq!(fake.commands(), vec![command.clone(), command]);
    }
}
