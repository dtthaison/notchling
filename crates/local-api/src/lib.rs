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
mod server_audit_reason_tests;
#[cfg(test)]
mod server_audit_tests;
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

pub const SOCKET_NAME: &str = "notchlingd.sock";
pub const AUDIT_NAME: &str = "audit.jsonl";

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
