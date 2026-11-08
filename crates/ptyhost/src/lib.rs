//! `ptyhost`: the session host, a port of the Swift `agent-console-hostd` with its socket contract.
//! It owns ptys and the processes on them, so restarting the app (later: the daemon) never ends a
//! terminal session. Built and tested here, never installed: the Swift hostd keeps the LaunchAgent
//! (`local.agentconsole.hostd`) and its socket until the maintainer hands them over.
//!
//! # The contract
//!
//! - Socket: `~/Library/Application Support/AgentConsole/hostd.sock` (folder 0700, socket 0600),
//!   only the same uid may connect. State: `hostd-state.json` beside it.
//! - Frames ([`wire`]): `[u32 big-endian length][type][payload]`; type 1 JSON, type 2 a 16-byte session
//!   id then terminal bytes.
//! - Ops (`ops.rs`): `hello`, `status`, `list`, `spawn`, `resume`, `resize`, `attach`, `detach`,
//!   `terminate` (needs `confirm: true`), `settings.get`, `settings.set`. Events: `spawned`, `exit`.
//! - Command line: `--socket PATH`, `--state PATH`, `--status`, `--record-resume SESSION_ID CWD`.

pub mod error;
pub mod host;
pub mod state;
pub mod system;
pub mod wire;

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
mod ops;
#[cfg(unix)]
pub mod pty_handle;
#[cfg(unix)]
pub mod server;

pub use error::HostError;

/// The line `--version` prints: `<name> <version>`.
pub fn version_line() -> String {
    format!(
        "{} {}",
        env!("CARGO_PKG_NAME").trim_start_matches("notchling-"),
        env!("CARGO_PKG_VERSION")
    )
}

/// The host's folder: `~/Library/Application Support/AgentConsole` on macOS (where the Swift host
/// keeps it), `~/.local/share/AgentConsole` elsewhere.
pub fn support_dir() -> Option<std::path::PathBuf> {
    let home = system::home()?;
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/AgentConsole")
    } else {
        home.join(".local/share/AgentConsole")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_is_name_then_version() {
        assert_eq!(version_line(), "ptyhost 0.1.0");
    }
}
