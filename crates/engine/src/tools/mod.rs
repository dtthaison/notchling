//! E6: other tools. `acp` is the Agent Client Protocol (JSON-RPC lines over a child's stdio) and
//! `agy` Antigravity CLI over it. Every process is started through a trait ([`acp::Spawner`]), so
//! a test swaps in a scripted fake and no test runs `agy`.

pub mod acp;
pub mod agy;
