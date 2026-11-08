//! E5: the phone bridge (protocol `agentconsole.v1`, `protocol/README.md`).
//!
//! [`net`] is the transport (E5b): TLS on the LAN port, the tunnel origin on loopback, the
//! WebSocket wire, pairing, paired devices, the rate limiter, the address filter, the Cloudflare
//! Access check and the audit log. It hands each phone that has said `hello` (or paired) to a
//! [`net::BridgeHandler`], which the router (E5a) implements.

pub mod net;
