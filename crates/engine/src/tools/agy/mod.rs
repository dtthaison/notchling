//! Antigravity CLI (`agy`) driven over the Agent Client Protocol (Swift `AntigravityCLI.swift`,
//! `ACPRunner.swift`).
//!
//! | Part | What |
//! | --- | --- |
//! | [`summaries::rows`]`(root, copy_dir, now)` | The session list, read from a COPY of `conversation_summaries.db` and its `-wal` (the original is never opened). |
//! | [`Runner`] | One `agy acp` per opened session: `open`, `start`, `prompt`, `cancel`, `answer` (a permission), `set_option`, `close`, `reap` (10 minutes idle). |
//! | [`Sink`] / [`Change`] | What the runner reports: loaded, chunks, turn start and end, a permission waiting or resolved, options changed, closed. |
//! | [`config`] | The session's options, slash commands and a waiting [`Permission`]. |
//!
//! The conversation itself is [`crate::transcript::acp::Transcript`]. Nothing here starts a process
//! except through a [`crate::tools::acp::Spawner`], and nothing is auto-approved.

pub mod config;
mod live;
pub mod runner;
pub mod summaries;

pub use config::{AgyCommand, ConfigOption, ConfigValue, Permission};
pub use runner::{Change, Clock, RunError, Runner, Sink, Timing};

use std::path::{Path, PathBuf};

/// The `agy` binary: the installer's `~/.local/bin`, else Homebrew's.
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
