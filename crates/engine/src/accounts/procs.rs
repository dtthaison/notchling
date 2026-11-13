use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub trait LoginChild: Send {
    fn output(&mut self) -> String;
    fn exit_code(&mut self) -> Option<i32>;
    fn kill(&mut self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginSpawn {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub log: PathBuf,
}

pub trait Procs: Send + Sync {
    fn capture(&self, program: &str, args: &[&str], timeout: Duration) -> Option<String>;
    fn run_null(&self, program: &Path, args: &[&str], timeout: Duration) -> Result<(), String>;
    fn spawn_login(&self, spawn: &LoginSpawn) -> Result<Box<dyn LoginChild>, String>;
    fn find_binary(&self, name: &str) -> Option<PathBuf>;
}

pub trait Apps: Send + Sync {
    fn is_running(&self, bundle_id: &str) -> bool;
    fn quit(&self, bundle_id: &str) -> Result<bool, DidNotQuit>;
    fn open(&self, bundle_id: &str);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DidNotQuit;

pub const QUIT_WAIT: Duration = Duration::from_secs(15);

pub struct SystemProcs {
    pub home: PathBuf,
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcCall {
    Capture(String),
    RunNull(String),
    SpawnLogin(LoginSpawn),
    FindBinary(String),
}

#[derive(Default)]
pub struct FakeProcs {
    captures: Mutex<HashMap<String, String>>,
    binaries: Mutex<HashMap<String, PathBuf>>,
    calls: Mutex<Vec<ProcCall>>,
    login: Mutex<Option<Arc<Mutex<FakeLoginState>>>>,
    run_null_fails: Mutex<bool>,
}

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

    pub fn stubborn(self, bundle_id: &str) -> FakeApps {
        lock(&self.stubborn).insert(bundle_id.to_string());
        self
    }

    pub fn when_quitting(&self, f: impl Fn(&str) + Send + 'static) {
        *lock(&self.on_quit) = Some(Box::new(f));
    }

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
