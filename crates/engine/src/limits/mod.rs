pub mod claude;
pub mod logic;
pub mod mode;
pub mod provider;
pub mod report;
pub mod sources;
pub mod text;
pub mod usage;

#[cfg(test)]
mod claude_tests;
#[cfg(test)]
mod logic_tests;
#[cfg(test)]
mod mode_tests;
#[cfg(test)]
mod provider_tests;
#[cfg(test)]
mod usage_tests;

pub use claude::{ClaudeWatcher, Observed};
pub use mode::{LimitConfig, LimitMode};
pub use provider::{ProviderLimitWatcher, Setup};
pub use report::UsageReport;
pub use usage::{parse_usage, AccountUsage, UsageWindow};
