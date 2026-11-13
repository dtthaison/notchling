use crate::control::plan::PtyScript;
use crate::control::{Command, LaunchError, LaunchOutput, Launcher};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const SAFE_ERROR: &str = "can't read this repo safely";

#[cfg(target_os = "macos")]
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
#[cfg(target_os = "macos")]
const SYSTEM_GIT: &str = "/usr/bin/git";
#[cfg(target_os = "macos")]
const TOOLS_GIT: &str = "/Library/Developer/CommandLineTools/usr/bin/git";
#[cfg(target_os = "macos")]
const SANDBOX_USAGE_ERROR: i32 = 71;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Resolved {
    profile: String,
    program: String,
}

pub struct SandboxedLauncher {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    inner: Arc<dyn Launcher>,
    resolved: OnceLock<Result<Resolved, String>>,
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
fn usable(path: &str) -> bool {
    !path.is_empty() && !path.contains(['\0', '\n'])
}

#[cfg(target_os = "macos")]
fn allow(programs: &mut Vec<String>, folders: &mut Vec<String>, path: &Path) {
    let mut forms = vec![path.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(path) {
        forms.push(real);
    }
    for form in forms {
        if let Some(text) = form.to_str().filter(|t| usable(t)) {
            if !programs.iter().any(|p| p == text) {
                programs.push(text.to_owned());
            }
        }
        let core = form
            .parent()
            .and_then(Path::parent)
            .map(|prefix| prefix.join("libexec").join("git-core"));
        if let Some(core) = core.filter(|c| c.is_dir()) {
            if let Some(text) = core.to_str().filter(|t| usable(t)) {
                if !folders.iter().any(|f| f == text) {
                    folders.push(text.to_owned());
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn homebrew_git() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("git");
        if !candidate.is_file() {
            continue;
        }
        let real = std::fs::canonicalize(&candidate).ok()?;
        let text = real.to_str()?;
        return (text.starts_with("/opt/homebrew/") || text.starts_with("/usr/local/Cellar/"))
            .then_some(real);
    }
    None
}

#[cfg(target_os = "macos")]
fn resolve(inner: &dyn Launcher) -> Result<Resolved, String> {
    if !Path::new(SANDBOX_EXEC).is_file() {
        return Err("sandbox-exec is missing".to_owned());
    }
    if !Path::new(SYSTEM_GIT).is_file() {
        return Err("there is no /usr/bin/git".to_owned());
    }
    let (mut programs, mut folders) = (Vec::new(), Vec::new());
    allow(&mut programs, &mut folders, Path::new(SYSTEM_GIT));
    let found = inner.run(
        &Command::new(&["/usr/bin/xcrun", "--find", "git"]),
        Duration::from_secs(5),
    );
    if let Ok(out) = found {
        let target = out.output.trim();
        if out.code == 0 && usable(target) && Path::new(target).is_file() {
            allow(&mut programs, &mut folders, Path::new(target));
        }
    }
    if Path::new(TOOLS_GIT).is_file() {
        allow(&mut programs, &mut folders, Path::new(TOOLS_GIT));
    }
    if let Some(brew) = homebrew_git() {
        allow(&mut programs, &mut folders, &brew);
    }
    Ok(Resolved {
        profile: profile(&programs, &folders),
        program: SYSTEM_GIT.to_owned(),
    })
}

impl SandboxedLauncher {
    pub fn new(inner: Arc<dyn Launcher>) -> SandboxedLauncher {
        SandboxedLauncher {
            inner,
            resolved: OnceLock::new(),
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
            .resolved
            .get_or_init(|| resolve(self.inner.as_ref()))
            .as_ref()
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
        let _ = &self.resolved;
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
