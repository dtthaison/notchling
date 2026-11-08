//! A program on a pseudo-terminal, started with `forkpty` (`TerminalSession.swift`'s start, and the
//! recorder's `Capture`). The pty becomes the child's controlling terminal the way a terminal app's
//! does, so a resize reaches it as SIGWINCH and it repaints; with posix_spawn, `claude attach` got the
//! signal but never repainted. macOS and Linux only; elsewhere [`Pty::spawn`] is
//! [`PtyError::Unsupported`].

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

/// The variable that is never passed on: an inherited `NO_COLOR` left Claude Code's menus all one gray.
pub const DROPPED_VARIABLE: &str = "NO_COLOR";

/// What to run on the pty. The environment starts as this process's, with `TERM=xterm-256color` (what
/// both Swift callers set); `NO_COLOR` is dropped at spawn whatever the caller set.
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
    /// `program` with no arguments, at `ReplySender`'s attach size, 50×200.
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

    /// The folder the child starts in; this process's when unset.
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

    /// Starts from an empty environment (then `env` adds to it).
    pub fn env_clear(&mut self) -> &mut PtyCommand {
        self.env.clear();
        self
    }

    /// The window size the child starts with. Zero counts as one.
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

    /// The environment the child gets: the command's, without [`DROPPED_VARIABLE`].
    pub fn child_env(&self) -> impl Iterator<Item = (&OsString, &OsString)> {
        self.env
            .iter()
            .filter(|(k, _)| k.as_os_str() != DROPPED_VARIABLE)
    }
}

/// Why a pty call failed.
#[derive(Debug)]
pub enum PtyError {
    /// No pty on this system yet (anything but macOS and Linux).
    Unsupported,
    /// A program, argument, variable or folder has a NUL byte in it.
    Nul,
    /// `forkpty` failed.
    Spawn(std::io::Error),
    /// Reading, writing, resizing or waiting failed.
    Io(std::io::Error),
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtyError::Unsupported => write!(f, "a terminal isn't supported on this system yet"),
            PtyError::Nul => write!(f, "a NUL byte in the command"),
            PtyError::Spawn(e) => write!(f, "couldn't open a terminal: {e}"),
            PtyError::Io(e) => write!(f, "terminal: {e}"),
        }
    }
}

impl std::error::Error for PtyError {}

/// What one read got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// This many bytes.
    Data(usize),
    /// Nothing within the time given.
    Timeout,
    /// The child's side is closed: it ended, and everything it wrote has been read.
    Closed,
}

/// A signal to send the child.
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
