//! The processes and apps the providers start and stop, behind traits so no test runs one:
//!
//! - [`Procs`]: read a command's output (`ps`, `pgrep`, `lsof`), run one with every stream sent to
//!   /dev/null (`codex app-server daemon restart`: a daemon that keeps a pipe open would make a
//!   read wait forever), start `codex login` with its output in a file, find a tool's binary.
//! - [`Apps`]: is an app running, quit it and wait, open it again.
//!
//! The real ones are [`SystemProcs`] and [`SystemApps`]; the fakes record every call.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A long-lived child whose output goes to a file (`codex login`).
pub trait LoginChild: Send {
    /// What it has printed so far.
    fn output(&mut self) -> String;
    /// Its exit code once it has ended (-1 when a signal ended it).
    fn exit_code(&mut self) -> Option<i32>;
    fn kill(&mut self);
}

/// What to start for a sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginSpawn {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Added to the daemon's own environment.
    pub env: Vec<(String, String)>,
    /// Where its standard output and error go (created 0600).
    pub log: PathBuf,
}

pub trait Procs: Send + Sync {
    /// Standard output of a command that finishes within `timeout`; `None` when it can't start or
    /// runs out of time. Standard input and error are /dev/null.
    fn capture(&self, program: &str, args: &[&str], timeout: Duration) -> Option<String>;
    /// Runs a command with all three streams on /dev/null and waits up to `timeout`, then ends it.
    fn run_null(&self, program: &Path, args: &[&str], timeout: Duration) -> Result<(), String>;
    fn spawn_login(&self, spawn: &LoginSpawn) -> Result<Box<dyn LoginChild>, String>;
    /// The tool's command: the usual install places, else the login shell's PATH.
    fn find_binary(&self, name: &str) -> Option<PathBuf>;
}

/// The apps on this computer.
pub trait Apps: Send + Sync {
    fn is_running(&self, bundle_id: &str) -> bool;
    /// Quits the app and waits for it: `Ok(true)` if it was running and is gone, `Ok(false)` if it
    /// wasn't running, `Err` if it is still running after the wait.
    fn quit(&self, bundle_id: &str) -> Result<bool, DidNotQuit>;
    fn open(&self, bundle_id: &str);
}

/// An app was asked to quit and was still running after the wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DidNotQuit;

/// How long an app is given to quit (`quitApp`).
pub const QUIT_WAIT: Duration = Duration::from_secs(15);

/// Real processes. `home` is the user's home folder, where the tools' install places are.
pub struct SystemProcs {
    pub home: PathBuf,
}

/// Real apps (macOS: the process table says what runs and when an app is gone,
/// `NSRunningApplication` asks it to quit, `open -b` opens it).
pub struct SystemApps;

mod real {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    fn wait_or_kill(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        }
    }

    impl Procs for SystemProcs {
        fn capture(&self, program: &str, args: &[&str], timeout: Duration) -> Option<String> {
            use std::io::Read;
            let mut child = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut out = child.stdout.take()?;
            let reader = std::thread::spawn(move || {
                let mut data = Vec::new();
                let _ = out.read_to_end(&mut data);
                data
            });
            wait_or_kill(&mut child, timeout)?;
            let data = reader.join().ok()?;
            Some(String::from_utf8_lossy(&data).into_owned())
        }

        fn run_null(&self, program: &Path, args: &[&str], timeout: Duration) -> Result<(), String> {
            let mut child = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "couldn't start it".to_string())?;
            // Past the timeout the command is ended, as the Swift `runQuietly` did.
            let _ = wait_or_kill(&mut child, timeout);
            Ok(())
        }

        fn spawn_login(&self, spawn: &LoginSpawn) -> Result<Box<dyn LoginChild>, String> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let log = options
                .open(&spawn.log)
                .map_err(|_| "couldn't make the log file".to_string())?;
            let err = log
                .try_clone()
                .map_err(|_| "couldn't make the log file".to_string())?;
            let child = Command::new(&spawn.program)
                .args(&spawn.args)
                .envs(spawn.env.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::null())
                .stdout(log)
                .stderr(err)
                .spawn()
                .map_err(|_| "couldn't start it".to_string())?;
            Ok(Box::new(RealLogin {
                child,
                log: spawn.log.clone(),
            }))
        }

        fn find_binary(&self, name: &str) -> Option<PathBuf> {
            let home = &self.home;
            let usual = [
                home.join(".local/bin").join(name),
                PathBuf::from("/opt/homebrew/bin").join(name),
                PathBuf::from("/usr/local/bin").join(name),
                home.join(".npm-global/bin").join(name),
            ];
            if let Some(found) = usual.into_iter().find(|p| is_executable(p)) {
                return Some(found);
            }
            if name.contains(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                return None;
            }
            let found = self.capture(
                "/bin/zsh",
                &["-lc", &format!("command -v {name}")],
                Duration::from_secs(5),
            )?;
            let path = PathBuf::from(found.trim());
            (path.is_absolute() && is_executable(&path)).then_some(path)
        }
    }

    pub(super) fn is_executable(path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            path.is_file()
        }
    }

    struct RealLogin {
        child: Child,
        log: PathBuf,
    }

    impl LoginChild for RealLogin {
        fn output(&mut self) -> String {
            std::fs::read(&self.log)
                .map(|d| String::from_utf8_lossy(&d).into_owned())
                .unwrap_or_default()
        }

        fn exit_code(&mut self) -> Option<i32> {
            self.child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| s.code().unwrap_or(-1))
        }

        fn kill(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl Apps for SystemApps {
        fn is_running(&self, bundle_id: &str) -> bool {
            notchling_platform::apps::is_running(bundle_id)
        }

        fn quit(&self, bundle_id: &str) -> Result<bool, DidNotQuit> {
            notchling_platform::apps::quit_and_wait(bundle_id, QUIT_WAIT).map_err(|_| DidNotQuit)
        }

        fn open(&self, bundle_id: &str) {
            if cfg!(target_os = "macos") {
                let _ = Command::new("/usr/bin/open")
                    .args(["-b", bundle_id])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map(|mut c| {
                        let _ = c.wait();
                    });
            }
        }
    }
}

/// What a [`FakeProcs`] was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcCall {
    Capture(String),
    RunNull(String),
    SpawnLogin(LoginSpawn),
    FindBinary(String),
}

/// Answers `capture` from a table keyed by `"<program> <args…>"`, records every call, and finds
/// binaries from a table.
#[derive(Default)]
pub struct FakeProcs {
    captures: Mutex<HashMap<String, String>>,
    binaries: Mutex<HashMap<String, PathBuf>>,
    calls: Mutex<Vec<ProcCall>>,
    login: Mutex<Option<Arc<Mutex<FakeLoginState>>>>,
    run_null_fails: Mutex<bool>,
}

/// What a fake sign-in has printed and how it ended; a test changes it while the flow waits.
#[derive(Default)]
pub struct FakeLoginState {
    pub output: String,
    pub exit: Option<i32>,
    pub killed: bool,
}

impl FakeProcs {
    pub fn new() -> FakeProcs {
        FakeProcs::default()
    }

    pub fn answer(self, command: &str, output: &str) -> FakeProcs {
        lock(&self.captures).insert(command.to_string(), output.to_string());
        self
    }

    pub fn binary(self, name: &str, path: &Path) -> FakeProcs {
        lock(&self.binaries).insert(name.to_string(), path.to_path_buf());
        self
    }

    pub fn set_run_null_fails(&self) {
        *lock(&self.run_null_fails) = true;
    }

    /// The state of the sign-in the next `spawn_login` returns.
    pub fn login_state(&self) -> Arc<Mutex<FakeLoginState>> {
        lock(&self.login)
            .get_or_insert_with(|| Arc::new(Mutex::new(FakeLoginState::default())))
            .clone()
    }

    pub fn calls(&self) -> Vec<ProcCall> {
        lock(&self.calls).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct FakeLogin(Arc<Mutex<FakeLoginState>>);

impl LoginChild for FakeLogin {
    fn output(&mut self) -> String {
        lock(&self.0).output.clone()
    }
    fn exit_code(&mut self) -> Option<i32> {
        lock(&self.0).exit
    }
    fn kill(&mut self) {
        let mut s = lock(&self.0);
        s.killed = true;
        s.exit.get_or_insert(-1);
    }
}

impl Procs for FakeProcs {
    fn capture(&self, program: &str, args: &[&str], _timeout: Duration) -> Option<String> {
        let key = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        lock(&self.calls).push(ProcCall::Capture(key.clone()));
        lock(&self.captures).get(&key).cloned()
    }

    fn run_null(&self, program: &Path, args: &[&str], _timeout: Duration) -> Result<(), String> {
        let key = std::iter::once(program.to_string_lossy().into_owned())
            .chain(args.iter().map(|a| a.to_string()))
            .collect::<Vec<_>>()
            .join(" ");
        lock(&self.calls).push(ProcCall::RunNull(key));
        if *lock(&self.run_null_fails) {
            return Err("couldn't start it".into());
        }
        Ok(())
    }

    fn spawn_login(&self, spawn: &LoginSpawn) -> Result<Box<dyn LoginChild>, String> {
        lock(&self.calls).push(ProcCall::SpawnLogin(spawn.clone()));
        Ok(Box::new(FakeLogin(self.login_state())))
    }

    fn find_binary(&self, name: &str) -> Option<PathBuf> {
        lock(&self.calls).push(ProcCall::FindBinary(name.to_string()));
        lock(&self.binaries).get(name).cloned()
    }
}

type QuitHook = Box<dyn Fn(&str) + Send>;

/// Which bundle ids are running; quitting removes one unless it is stubborn.
#[derive(Default)]
pub struct FakeApps {
    running: Mutex<HashSet<String>>,
    stubborn: Mutex<HashSet<String>>,
    events: Mutex<Vec<String>>,
    on_quit: Mutex<Option<QuitHook>>,
}

impl FakeApps {
    pub fn new() -> FakeApps {
        FakeApps::default()
    }

    pub fn running(self, bundle_id: &str) -> FakeApps {
        lock(&self.running).insert(bundle_id.to_string());
        self
    }

    /// This app never quits.
    pub fn stubborn(self, bundle_id: &str) -> FakeApps {
        lock(&self.stubborn).insert(bundle_id.to_string());
        self
    }

    /// Runs when an app is asked to quit, before it is gone: a test reads the files then to show
    /// nothing was written first.
    pub fn when_quitting(&self, f: impl Fn(&str) + Send + 'static) {
        *lock(&self.on_quit) = Some(Box::new(f));
    }

    /// `is_running`, `quit <id>` and `open <id>` calls, in order.
    pub fn events(&self) -> Vec<String> {
        lock(&self.events).clone()
    }
}

impl Apps for FakeApps {
    fn is_running(&self, bundle_id: &str) -> bool {
        lock(&self.events).push(format!("is_running {bundle_id}"));
        lock(&self.running).contains(bundle_id)
    }

    fn quit(&self, bundle_id: &str) -> Result<bool, DidNotQuit> {
        lock(&self.events).push(format!("quit {bundle_id}"));
        if !lock(&self.running).contains(bundle_id) {
            return Ok(false);
        }
        if let Some(f) = lock(&self.on_quit).as_ref() {
            f(bundle_id);
        }
        if lock(&self.stubborn).contains(bundle_id) {
            return Err(DidNotQuit);
        }
        lock(&self.running).remove(bundle_id);
        Ok(true)
    }

    fn open(&self, bundle_id: &str) {
        lock(&self.events).push(format!("open {bundle_id}"));
        lock(&self.running).insert(bundle_id.to_string());
    }
}
