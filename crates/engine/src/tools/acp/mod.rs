pub mod connection;
pub mod fake;
pub mod process;

pub use connection::{Connection, Done, Handler};
pub use process::{Killer, Output, RealSpawner, Spawned, Spawner, ToolProcess, MAX_LINE};

pub const METHOD_NOT_FOUND: i64 = -32601;
