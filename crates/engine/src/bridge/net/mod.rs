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
//!   stalls nobody else. Its text frames go to the router (E5a, [`crate::bridge::Router`]), each
//!   on a thread of its own as the Swift bridge's `Task`s, a few at a time per phone.
//! - [`core::Core`] holds the router, the hub and the stores it uses: paired phones in the Swift
//!   app's `devices.json` ([`devices`]), tokens compared in constant time; one-time pairing codes
//!   ([`pairing`]); a backoff per address for failed logins ([`limiter`]); the audit log
//!   ([`audit`]). [`parts`] fits them to the router's traits.
//! - [`bridge::Bridge`] is the switch: off binds nothing and writes nothing.

pub mod access;
pub mod address;
pub mod app_url;
pub mod audit;
pub mod bridge;
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
pub mod parts;
pub mod server;
mod wire;

#[cfg(test)]
mod e2e_tests;

pub use bridge::{Bridge, BridgeConfig};

/// The two ports, as the Swift app has them. A busy port is reported, never replaced.
pub mod ports {
    /// TLS with the pinned certificate, for the LAN and Tailscale.
    pub const LAN: u16 = 47821;
    /// Plain HTTP on 127.0.0.1 only, for cloudflared.
    pub const TUNNEL: u16 = 47822;
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_ports_are_fixed_and_the_tunnel_one_is_not_the_lan_one() {
        assert_eq!(super::ports::LAN, 47821);
        assert_eq!(super::ports::TUNNEL, 47822);
    }
}
