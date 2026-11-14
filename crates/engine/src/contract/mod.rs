pub mod antigravity;
pub mod baseline;
mod capability;
pub mod claude;
pub mod codex;
pub mod conformance;
mod provider;
mod registry;
mod support;
mod tool;

pub use capability::{Capability, Group};
pub use provider::Provider;
pub use registry::{Cell, Registry};
pub use support::{Flow, Kind, Support, Unsupported};
pub use tool::Tool;

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
#[cfg(test)]
#[path = "support_tests.rs"]
mod support_tests;
#[cfg(test)]
#[path = "tool_tests.rs"]
mod tool_tests;
