//! E17: the local API between `notchlingd` and its clients on the same computer (decision 0007).
//!
//! A Unix socket (a named pipe on Windows, not built yet: every call there says `Unsupported`),
//! one JSON object per line, at most 1 MiB a line ([`wire`], [`framing`]). The daemon knows each
//! caller by its executable path ([`peer`]) and gives it a [`Tier`] from an allowlist ([`tier`]):
//! the installed app may do everything, everything else is the agent tier. Refusals are written
//! to an audit log ([`audit`]).
//!
//! # For the daemon
//!
//! [`Server::bind`] (lock, stale socket, 0600) with a [`Handler`] that names each op's
//! [`OpSpec`]; [`Server::spawn`] or [`Server::run`]; [`ServerHandle::publish`] for events and
//! [`ServerHandle::shutdown`].
//!
//! # For the app and the command line
//!
//! [`Client::connect`] (says hello), [`Client::sessions`], [`Client::status`],
//! [`Client::subscribe`] then [`Client::next_event`] or [`Client::into_events`]. Calls block, so the
//! app makes them off its UI thread. [`ensure_daemon`] starts a missing daemon.

pub mod audit;
pub mod client;
mod conn;
pub mod framing;
mod hub;
pub mod lock;
pub mod peer;
pub mod server;
pub mod spawn;
pub mod tier;
pub mod transport;
pub mod wire;

#[cfg(test)]
mod server_tests;
#[cfg(test)]
pub(crate) mod testutil;

pub use audit::{AuditEntry, AuditLog};
pub use client::{server_trusted, Client, ClientError, ConnectOptions, CALL_TIMEOUT};
pub use framing::MAX_LINE;
pub use server::{Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle};
pub use spawn::{ensure_daemon, ensure_daemon_with, spawn_allowed, SpawnOptions, Started};
pub use tier::{Allowlist, APP_TIER_PATHS_ENV};
pub use wire::{
    ops, topics, ApiError, ClientKind, DaemonStatus, ErrorCode, Event, HelloReply, Tier,
    API_VERSION,
};

/// The socket's file name in the data folder.
pub const SOCKET_NAME: &str = "notchlingd.sock";
/// The audit log's file name in the data folder.
pub const AUDIT_NAME: &str = "audit.jsonl";

/// Windows' pipe name for a user: `\\.\pipe\notchlingd-<user>`.
pub fn pipe_name(user: &str) -> String {
    format!(r"\\.\pipe\notchlingd-{user}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipe_is_named_per_user() {
        assert_eq!(pipe_name("me"), r"\\.\pipe\notchlingd-me");
    }
}
