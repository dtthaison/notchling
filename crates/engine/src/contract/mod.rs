pub mod antigravity;
mod capability;
pub mod claude;
pub mod codex;
mod provider;
mod support;
mod tool;

pub use capability::{Capability, Group};
pub use provider::Provider;
pub use support::{Flow, Kind, Support, Unsupported};
pub use tool::Tool;

#[cfg(test)]
#[path = "support_tests.rs"]
mod support_tests;
#[cfg(test)]
#[path = "tool_tests.rs"]
mod tool_tests;
