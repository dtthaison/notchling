//! Starting processes. Everything that launches `claude` or `git` goes through [`Launcher`], so a
//! test swaps in [`FakeLauncher`] and no test ever runs a real session.

use super::plan::{Command, PtyScript};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
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
    delay: Mutex<Duration>,
}

impl FakeLauncher {
    pub fn new() -> Self {
        FakeLauncher::default()
    }

    /// Every `run` takes this long to answer (a slow command).
    pub fn set_delay(&self, delay: Duration) {
        *lock(&self.delay) = delay;
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
        let delay = *lock(&self.delay);
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
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
    /// Where the output files go: the system's temp folder unless a test names another.
    temp_dir: PathBuf,
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
        RealLauncher::with_claude(claude_path(home))
    }

    /// The `claude` this launcher runs.
    pub fn claude(&self) -> &Path {
        &self.claude
    }

    pub fn with_claude(path: PathBuf) -> Self {
        RealLauncher {
            claude: path,
            temp_dir: std::env::temp_dir(),
        }
    }

    /// Puts the output files in `dir` instead of the system's temp folder.
    pub fn with_temp_dir(mut self, dir: PathBuf) -> Self {
        self.temp_dir = dir;
        self
    }

    fn program(&self, name: &str) -> PathBuf {
        match name {
            "claude" => self.claude.clone(),
            "git" if Path::new("/usr/bin/git").is_file() => PathBuf::from("/usr/bin/git"),
            other => PathBuf::from(other),
        }
    }
}

/// Creates `path` for writing and reading back, owner-only (0600), and only if nothing is there: `O_EXCL` refuses an
/// existing file or link, and `O_NOFOLLOW` a link even where `O_EXCL` alone would not be checked.
fn create_exclusive(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

/// A name nobody can guess: `RandomState` is keyed from the operating system's randomness.
fn random_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(u64::from(std::process::id()));
    format!("{:016x}", hasher.finish())
}

/// One run's output files: standard output and standard error, both new, both 0600, under names made
/// of random digits. A name already taken is tried again with another.
struct OutputFiles {
    out: (File, PathBuf),
    err: (File, PathBuf),
}

impl OutputFiles {
    fn create(dir: &Path) -> std::io::Result<OutputFiles> {
        for _ in 0..8 {
            let stem = format!("notchling-{}", random_name());
            let out_path = dir.join(format!("{stem}.out"));
            let err_path = dir.join(format!("{stem}.err"));
            let out = match create_exclusive(&out_path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            match create_exclusive(&err_path) {
                Ok(err) => {
                    return Ok(OutputFiles {
                        out: (out, out_path),
                        err: (err, err_path),
                    })
                }
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(&out_path);
                    if e.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(e);
                    }
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "no unused name for an output file",
        ))
    }
}

impl Launcher for RealLauncher {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        let Some(name) = command.argv.first() else {
            return Err(LaunchError::Spawn("empty command".to_string()));
        };
        let files =
            OutputFiles::create(&self.temp_dir).map_err(|e| LaunchError::Spawn(e.to_string()))?;
        let (out_path, err_path) = (files.out.1.clone(), files.err.1.clone());
        let result = self.run_to_files(name, command, timeout, files);
        // Whatever happened, including a timeout, nothing is left behind.
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&err_path);
        result
    }

    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

impl RealLauncher {
    fn run_to_files(
        &self,
        name: &str,
        command: &Command,
        timeout: Duration,
        files: OutputFiles,
    ) -> Result<LaunchOutput, LaunchError> {
        let OutputFiles {
            out: (out, _),
            err: (err, _),
        } = files;
        // Read back through our own handles, not by path: a link swapped in at the path afterwards
        // must not make this read some other file. The offset is shared with the child's, so it is
        // rewound first.
        let (mut out_reader, mut err_reader) = (
            out.try_clone()
                .map_err(|e| LaunchError::Spawn(e.to_string()))?,
            err.try_clone()
                .map_err(|e| LaunchError::Spawn(e.to_string()))?,
        );
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
        Ok(LaunchOutput {
            code: status.code().unwrap_or(-1),
            output: read_back(&mut out_reader),
            stderr: read_back(&mut err_reader),
        })
    }
}

/// Everything the child wrote to its output file, from the start.
fn read_back(file: &mut File) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(0)).is_ok() {
        let _ = file.read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
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

    /// The files in `dir`, with their modes, while a slow command runs through a launcher that keeps
    /// its output there.
    fn files_while_running(dir: &Path) -> Vec<(PathBuf, u32)> {
        use std::os::unix::fs::PermissionsExt;
        let launcher = real().with_temp_dir(dir.to_path_buf());
        let runner = std::thread::spawn(move || {
            launcher.run(&Command::new(&["/bin/sleep", "1"]), Duration::from_secs(10))
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while seen.len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            seen = std::fs::read_dir(dir)
                .expect("dir")
                .flatten()
                .filter_map(|e| {
                    let mode = e.metadata().ok()?.permissions().mode() & 0o777;
                    Some((e.path(), mode))
                })
                .collect();
        }
        runner.join().expect("thread").expect("ran");
        seen
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-launcher-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// The output files hold whatever `claude` printed (a job's name, a prompt, an error with a path
    /// in it), so they are the owner's alone: mode 0600, not 0644.
    #[test]
    fn the_output_files_are_private_to_the_owner() {
        let dir = scratch("mode");
        let files = files_while_running(&dir);
        assert_eq!(files.len(), 2, "{files:?}");
        for (file, mode) in &files {
            assert_eq!(*mode, 0o600, "{}", file.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `notchling-<pid>-<n>.out` could be guessed, and a link planted at the guess followed.
    #[test]
    fn the_output_file_names_cannot_be_guessed() {
        let dir = scratch("names");
        let pid = std::process::id().to_string();
        let first = files_while_running(&dir);
        let second = files_while_running(&dir);
        for (file, _) in first.iter().chain(&second) {
            let name = file.file_name().expect("name").to_string_lossy();
            assert!(!name.contains(&pid), "{name} is built from the pid");
        }
        assert!(
            first
                .iter()
                .all(|(f, _)| second.iter().all(|(g, _)| f != g)),
            "a name was used twice: {first:?} {second:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing is left behind, even by a command that was killed at the timeout.
    #[test]
    fn the_output_files_are_removed_after_a_timeout_too() {
        let dir = scratch("cleanup");
        let launcher = real().with_temp_dir(dir.clone());
        let slow = Command::new(&["/bin/sleep", "5"]);
        assert!(launcher.run(&slow, Duration::from_millis(200)).is_err());
        assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file already at the path (or a link planted there) is never opened.
    #[test]
    fn an_existing_file_or_symlink_is_not_opened() {
        let dir = scratch("exclusive");
        let victim = dir.join("victim");
        std::fs::write(&victim, b"precious").expect("write");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&victim, &link).expect("link");
        assert!(create_exclusive(&link).is_err());
        assert!(create_exclusive(&victim).is_err());
        assert_eq!(std::fs::read(&victim).expect("read"), b"precious");
        let _ = std::fs::remove_dir_all(&dir);
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
