pub mod config;
pub mod elsewhere;
mod live;
pub mod runner;
pub mod summaries;

pub use config::{AgyCommand, ConfigOption, ConfigValue, Permission};
pub use runner::{Change, Clock, RunError, Runner, Sink, Timing};

use std::path::{Path, PathBuf};

pub fn find_binary(home: &Path) -> Option<PathBuf> {
    [
        home.join(".local/bin/agy"),
        PathBuf::from("/opt/homebrew/bin/agy"),
        PathBuf::from("/usr/local/bin/agy"),
    ]
    .into_iter()
    .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
