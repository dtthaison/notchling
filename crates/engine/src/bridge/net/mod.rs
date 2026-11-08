//! E5b: the phone bridge's transport, ported from ConsoleCore's `Bridge/` and Notchling's `Phone/`
//! (`BridgeServer`, `TunnelOriginServer`, `BridgeIdentity`, `BridgeController`).
//!
//! - The LAN port (47821 by default) is TLS with the Swift app's own certificate, read byte for
//!   byte from the data folder ([`identity`]), so a paired phone's pinned fingerprint still
//!   matches. Only this computer, the private LAN, link-local and Tailscale may connect
//!   ([`address`]).
//! - The tunnel origin (47822) is plain HTTP on loopback for cloudflared, behind the Cloudflare
//!   Access check ([`access`], [`jwt`]).
//! - Both speak RFC 6455 by hand ([`http`], [`frame`]): a head bounded in size and time (a slow
//!   client is answered 408), messages capped at 256 KB (1009), every refusal a real status.
//! - Each phone has its own reader and writer threads ([`server`]), so a slow or hostile phone
//!   stalls nobody else.
//! - Before `hello` or `pair`, [`core::Core`] answers; after it, every text frame goes to the
//!   [`BridgeHandler`] (the router). Paired phones live in the Swift app's `devices.json`
//!   ([`devices`]), tokens compared in constant time; pairing codes are one-time ([`pairing`]);
//!   failed logins back off per address ([`limiter`]); changes are audited ([`audit`]).
//! - [`bridge::Bridge`] is the switch: off binds nothing and writes nothing.

pub mod access;
pub mod address;
pub mod app_url;
pub mod audit;
pub mod bridge;
pub mod connection;
pub mod core;
pub mod crypto;
pub mod devices;
pub mod fetch;
pub mod files;
pub mod frame;
pub mod http;
pub mod identity;
pub mod jwt;
pub mod limiter;
pub mod pairing;
pub mod server;
mod wire;

pub use bridge::{Bridge, BridgeConfig};
pub use connection::{Connection, ConnectionIo};

/// `agentconsole.v1`'s `version` field.
pub const WIRE_VERSION: u32 = 1;

/// The two ports, as the Swift app has them. A busy port is reported, never replaced.
pub mod ports {
    /// TLS with the pinned certificate, for the LAN and Tailscale.
    pub const LAN: u16 = 47821;
    /// Plain HTTP on 127.0.0.1 only, for cloudflared.
    pub const TUNNEL: u16 = 47822;
}

/// What the transport hands a phone to once it has said `hello` with a paired token, or paired.
/// The router (E5a) implements it; the transport has already answered `hello` and `pair`.
pub trait BridgeHandler: Send + Sync {
    /// The phone is authenticated; the hello or pair reply has gone out.
    fn opened(&self, _connection: &std::sync::Arc<Connection>) {}
    /// One text frame (at most 256 KB, valid UTF-8). Replies go out through
    /// [`Connection::send_text`]; this runs on the phone's own reader thread.
    fn message(&self, connection: &std::sync::Arc<Connection>, text: &str);
    /// The connection is gone (closed, broken, revoked, or the bridge stopped).
    fn closed(&self, _connection: &std::sync::Arc<Connection>) {}
}

/// Asks the person at the computer whether a phone that gave the right code may pair. Blocks the
/// pairing phone's thread only. Answers false when nobody can answer.
pub trait PairApprover: Send + Sync {
    fn approve(&self, name: &str, platform: &str) -> bool;
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_ports_are_fixed_and_the_tunnel_one_is_not_the_lan_one() {
        assert_eq!(super::ports::LAN, 47821);
        assert_eq!(super::ports::TUNNEL, 47822);
    }
}
