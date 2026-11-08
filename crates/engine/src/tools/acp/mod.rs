//! The Agent Client Protocol (Swift `ACPConnection`): JSON-RPC 2.0, one message per line on a
//! child's stdin and stdout.
//!
//! - [`process`]: [`ToolProcess`] (write a line, kill) and [`Spawner`] (start one), the real child
//!   process with bounded line reading, and what a framing fault looks like ([`Output`]).
//! - [`connection`]: [`Connection`], requests with callbacks, the agent's own requests handed to a
//!   [`Handler`] (ids may be strings or numbers and are answered as they came), `-32601` for what
//!   isn't supported, and a dead process failing everything that waited.
//! - [`fake`]: a scripted process for tests, including a replay of a recording.
//!
//! Framing faults never reach a handler and never stop the connection: a line over
//! [`process::MAX_LINE`] is dropped as [`Output::Oversize`], a line that isn't a JSON object is
//! skipped, and a response to an id nobody waits for is ignored.

pub mod connection;
pub mod fake;
pub mod process;

pub use connection::{Connection, Done, Handler};
pub use process::{Output, RealSpawner, Spawned, Spawner, ToolProcess, MAX_LINE};

/// The JSON-RPC error code for a method this client doesn't offer.
pub const METHOD_NOT_FOUND: i64 = -32601;
