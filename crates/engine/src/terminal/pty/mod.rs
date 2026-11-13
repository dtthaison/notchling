use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use unix::Pty;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod other;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use other::Pty;

pub const DROPPED_VARIABLE: &str = "NO_COLOR";

#[derive(Clone, Debug)]
pub struct PtyCommand {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) env: BTreeMap<OsString, OsString>,
    pub(crate) rows: u16,
    pub(crate) cols: u16,
}

impl PtyCommand {
    pub fn new(program: impl AsRef<Path>) -> PtyCommand {
        let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        env.insert("TERM".into(), "xterm-256color".into());
        PtyCommand {
            program: program.as_ref().to_path_buf(),
            args: Vec::new(),
            cwd: None,
            env,
            rows: 50,
            cols: 200,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut PtyCommand {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut PtyCommand
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    pub fn cwd(&mut self, dir: impl AsRef<Path>) -> &mut PtyCommand {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut PtyCommand {
        self.env
            .insert(key.as_ref().to_os_string(), value.as_ref().to_os_string());
        self
    }

    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut PtyCommand {
        self.env.remove(key.as_ref());
        self
    }

    pub fn env_clear(&mut self) -> &mut PtyCommand {
        self.env.clear();
        self
    }

    pub fn size(&mut self, rows: u16, cols: u16) -> &mut PtyCommand {
        self.rows = rows.max(1);
        self.cols = cols.max(1);
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn child_env(&self) -> impl Iterator<Item = (&OsString, &OsString)> {
        self.env
            .iter()
            .filter(|(k, _)| k.as_os_str() != DROPPED_VARIABLE)
    }
}

#[derive(Debug)]
pub enum PtyError {
    Unsupported,
    Nul,
    Spawn(std::io::Error),
    Start {
        step: StartStep,
        error: std::io::Error,
    },
    Io(std::io::Error),
    Timeout,
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtyError::Unsupported => write!(f, "a terminal isn't supported on this system yet"),
            PtyError::Nul => write!(f, "a NUL byte in the command"),
            PtyError::Spawn(e) => write!(f, "couldn't open a terminal: {e}"),
            PtyError::Start { step, error } => write!(f, "couldn't {step}: {error}"),
            PtyError::Io(e) => write!(f, "terminal: {e}"),
            PtyError::Timeout => write!(f, "the program isn't reading its input"),
        }
    }
}

impl std::error::Error for PtyError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartStep {
    Terminal,
    Folder,
    Program,
}

impl fmt::Display for StartStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StartStep::Terminal => "set up the terminal",
            StartStep::Folder => "enter the folder",
            StartStep::Program => "run the program",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Data(usize),
    Timeout,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
    Int,
    Hup,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_never_reaches_the_child() {
        let mut command = PtyCommand::new("/bin/sh");
        command.env("NO_COLOR", "1").env("A", "b");
        let env: Vec<_> = command.child_env().map(|(k, _)| k.clone()).collect();
        assert!(!env.contains(&OsString::from("NO_COLOR")));
        assert!(env.contains(&OsString::from("A")));
        assert!(env.contains(&OsString::from("TERM")));
    }

    #[test]
    fn size_is_at_least_one_cell() {
        let mut command = PtyCommand::new("/bin/sh");
        assert_eq!((command.rows(), command.cols()), (50, 200));
        command.size(0, 0);
        assert_eq!((command.rows(), command.cols()), (1, 1));
    }
}
