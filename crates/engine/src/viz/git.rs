//! Read-only git for the Flow tab (ConsoleCore's `GitReader`): the branch from `.git/HEAD`, and
//! `git diff --stat`, the only git command the Flow tab runs. Optional locks are off, so even
//! refreshing the index never writes. The process goes through a [`Launcher`].

use super::engine::GitSnapshot;
use crate::control::{Command, Launcher};
use std::path::Path;
use std::time::Duration;

/// The longest diff stat kept.
const MAX_STAT_BYTES: usize = 16_000;
const TIMEOUT: Duration = Duration::from_secs(3);

/// The command the Flow tab runs in `cwd`.
pub fn diff_stat_command(cwd: &str) -> Command {
    Command::new(&[
        "git",
        "--no-optional-locks",
        "-C",
        cwd,
        "diff",
        "--stat",
        "--no-color",
        "--no-ext-diff",
    ])
    .with_env("GIT_TERMINAL_PROMPT", Some("0"))
    .with_env("GIT_PAGER", Some("cat"))
}

/// An empty snapshot for a folder that is not there; the branch alone when git does not answer.
pub fn read(cwd: &str, launcher: &dyn Launcher) -> GitSnapshot {
    if cwd.is_empty() || !Path::new(cwd).exists() {
        return GitSnapshot::default();
    }
    let stat = launcher
        .run(&diff_stat_command(cwd), TIMEOUT)
        .ok()
        .filter(|out| out.code == 0)
        .map(|out| clip(&out.output))
        .filter(|text| !text.is_empty());
    GitSnapshot::with_branch(cwd, stat)
}

/// The first 16 000 bytes (cut on a character), trimmed.
fn clip(text: &str) -> String {
    let mut end = text.len().min(MAX_STAT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim().to_string()
}
