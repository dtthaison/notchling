use crate::control::plan::PtyScript;
use crate::control::{Command, LaunchError, LaunchOutput, Launcher};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const SAFE_ERROR: &str = "can't read this repo safely";

#[cfg(target_os = "macos")]
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
#[cfg(target_os = "macos")]
const TOOLS_GIT: &str = "/Library/Developer/CommandLineTools/usr/bin/git";
#[cfg(target_os = "macos")]
const SANDBOX_USAGE_ERROR: i32 = 71;
const FIRST_CALL_WAITS: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const RETRY_EVERY: Duration = Duration::from_secs(60);
#[cfg(target_os = "macos")]
const XCRUN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    pub mode: u32,
}

#[cfg(any(target_os = "macos", test))]
pub fn root_owned_chain(
    path: &std::path::Path,
    stat: &dyn Fn(&std::path::Path) -> Option<Owner>,
) -> bool {
    if !path.is_absolute() {
        return false;
    }
    path.ancestors()
        .all(|folder| stat(folder).is_some_and(|owner| owner.uid == 0 && owner.mode & 0o022 == 0))
}

#[cfg(any(target_os = "macos", test))]
fn quoted(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(any(target_os = "macos", test))]
pub fn profile(programs: &[String], folders: &[String]) -> String {
    let mut text = String::from("(version 1)\n(allow default)\n(deny process-exec*)\n");
    if programs.is_empty() && folders.is_empty() {
        return text;
    }
    text.push_str("(allow process-exec\n");
    for program in programs {
        text.push_str(&format!("  (literal {})\n", quoted(program)));
    }
    for folder in folders {
        text.push_str(&format!("  (subpath {})\n", quoted(folder)));
    }
    text.push_str(")\n");
    text
}

#[cfg(target_os = "macos")]
pub use mac::trusted;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::sync::{Condvar, Mutex, MutexGuard};
    use std::time::Instant;

    pub struct Resolved {
        pub profile: String,
        pub program: String,
    }

    enum State {
        Resolving,
        Ready(Arc<Resolved>),
        Failed { at: Instant, why: String },
    }

    struct Shared {
        state: Mutex<State>,
        changed: Condvar,
    }

    pub struct Resolver {
        shared: Arc<Shared>,
        inner: Arc<dyn Launcher>,
    }

    fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn real_owner(path: &Path) -> Option<Owner> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Owner {
            uid: meta.uid(),
            mode: meta.mode(),
        })
    }

    pub fn trusted(path: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(path).ok()?;
        let text = real.to_str()?;
        if text.contains(['\0', '\n']) {
            return None;
        }
        root_owned_chain(&real, &real_owner).then_some(real)
    }

    fn resolve(inner: &dyn Launcher) -> Result<Resolved, String> {
        if !Path::new(SANDBOX_EXEC).is_file() {
            return Err("sandbox-exec is missing".to_owned());
        }
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(out) = inner.run(
            &Command::new(&["/usr/bin/xcrun", "--find", "git"]),
            XCRUN_TIMEOUT,
        ) {
            let target = out.output.trim();
            if out.code == 0 && !target.is_empty() {
                candidates.push(PathBuf::from(target));
            }
        }
        candidates.push(PathBuf::from(TOOLS_GIT));
        let (mut programs, mut folders) = (Vec::<String>::new(), Vec::<String>::new());
        for candidate in &candidates {
            let Some(real) = trusted(candidate) else {
                continue;
            };
            let Some(text) = real.to_str() else {
                continue;
            };
            if !programs.iter().any(|p| p == text) {
                programs.push(text.to_owned());
            }
            let core = real
                .parent()
                .and_then(Path::parent)
                .map(|prefix| prefix.join("libexec").join("git-core"));
            if let Some(core) = core.and_then(|c| trusted(&c)) {
                if let Some(text) = core.to_str().filter(|_| core.is_dir()) {
                    if !folders.iter().any(|f| f == text) {
                        folders.push(text.to_owned());
                    }
                }
            }
        }
        let Some(program) = programs.first().cloned() else {
            return Err("no git binary that only root can change was found".to_owned());
        };
        Ok(Resolved {
            profile: profile(&programs, &folders),
            program,
        })
    }

    fn spawn_resolution(shared: &Arc<Shared>, inner: &Arc<dyn Launcher>) {
        let (shared, inner) = (shared.clone(), inner.clone());
        std::thread::spawn(move || {
            let result = resolve(inner.as_ref());
            let mut state = lock(&shared.state);
            *state = match result {
                Ok(resolved) => State::Ready(Arc::new(resolved)),
                Err(why) => State::Failed {
                    at: Instant::now(),
                    why,
                },
            };
            shared.changed.notify_all();
        });
    }

    impl Resolver {
        pub fn start(inner: Arc<dyn Launcher>) -> Resolver {
            let shared = Arc::new(Shared {
                state: Mutex::new(State::Resolving),
                changed: Condvar::new(),
            });
            spawn_resolution(&shared, &inner);
            Resolver { shared, inner }
        }

        pub fn current(&self, wait: Duration) -> Result<Arc<Resolved>, String> {
            let deadline = Instant::now() + wait;
            let mut state = lock(&self.shared.state);
            if let State::Failed { at, .. } = &*state {
                if at.elapsed() >= RETRY_EVERY {
                    *state = State::Resolving;
                    spawn_resolution(&self.shared, &self.inner);
                }
            }
            loop {
                match &*state {
                    State::Ready(resolved) => return Ok(resolved.clone()),
                    State::Failed { why, .. } => return Err(why.clone()),
                    State::Resolving => {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return Err("still starting".to_owned());
                        }
                        state = self
                            .shared
                            .changed
                            .wait_timeout(state, left)
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                }
            }
        }
    }
}

pub struct SandboxedLauncher {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    inner: Arc<dyn Launcher>,
    #[cfg(target_os = "macos")]
    resolver: mac::Resolver,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    wait: Duration,
}

impl SandboxedLauncher {
    pub fn new(inner: Arc<dyn Launcher>) -> SandboxedLauncher {
        SandboxedLauncher::waiting(inner, FIRST_CALL_WAITS)
    }

    pub fn waiting(inner: Arc<dyn Launcher>, wait: Duration) -> SandboxedLauncher {
        SandboxedLauncher {
            #[cfg(target_os = "macos")]
            resolver: mac::Resolver::start(inner.clone()),
            inner,
            wait,
        }
    }

    #[cfg(target_os = "macos")]
    fn wrapped(&self, command: &Command) -> Result<Command, LaunchError> {
        if command.argv.first().map(String::as_str) != Some("git") {
            return Err(LaunchError::Spawn(format!(
                "{SAFE_ERROR}: only git runs here"
            )));
        }
        let resolved = self
            .resolver
            .current(self.wait)
            .map_err(|why| LaunchError::Spawn(format!("{SAFE_ERROR}: {why}")))?;
        let mut argv = vec![
            SANDBOX_EXEC.to_owned(),
            "-p".to_owned(),
            resolved.profile.clone(),
            resolved.program.clone(),
        ];
        argv.extend(command.argv.iter().skip(1).cloned());
        Ok(Command {
            argv,
            env: command.env.clone(),
            cwd: command.cwd.clone(),
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn wrapped(&self, _command: &Command) -> Result<Command, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn checked(out: LaunchOutput) -> Result<LaunchOutput, LaunchError> {
    if out.code == SANDBOX_USAGE_ERROR && out.stderr.starts_with("sandbox-exec:") {
        let line = out.stderr.lines().next().unwrap_or("").trim();
        return Err(LaunchError::Spawn(format!("{SAFE_ERROR}: {line}")));
    }
    Ok(out)
}

#[cfg(not(target_os = "macos"))]
fn checked(out: LaunchOutput) -> Result<LaunchOutput, LaunchError> {
    Ok(out)
}

impl Launcher for SandboxedLauncher {
    fn run(&self, command: &Command, timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        checked(self.inner.run(&self.wrapped(command)?, timeout)?)
    }

    fn run_capped(
        &self,
        command: &Command,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<(LaunchOutput, bool), LaunchError> {
        let (out, cut) = self
            .inner
            .run_capped(&self.wrapped(command)?, timeout, max_bytes)?;
        Ok((checked(out)?, cut))
    }

    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}
