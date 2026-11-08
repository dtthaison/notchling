//! E8c: usage limits and the auto-switch (Milestone 6), ported from `LimitWatcher.swift`,
//! `ProviderLimits.swift` and the limits parts of `Accounts.swift`. Built and switched off: the
//! daemon runs the watchers only while its `accounts` switch is on.
//!
//! - [`usage`]: `/api/oauth/usage` into [`usage::AccountUsage`] (`apply(limits:)`, with the fixed
//!   keys as a fallback; `five_hour` may be null).
//! - [`report`]: [`report::UsageReport`], what every provider reports.
//! - [`logic`]: `LimitLogic`, pure: the 90/100 steps, `pick_target`, `may_switch`,
//!   `blocked_until`, `decide_switch`, [`logic::Candidate`].
//! - [`mode`]: [`mode::LimitMode`] per provider with the Swift setting keys and defaults, and
//!   [`mode::LimitConfig`], where the daemon keeps them.
//! - [`sources`]: the traits the watchers need from the account stores; the daemon implements
//!   them over the real stores.
//! - [`claude`]: [`claude::ClaudeWatcher`] (main login, lanes, jobs stopped on the limit).
//! - [`provider`]: [`provider::ProviderLimitWatcher`] (Codex: Notify by default, switches only
//!   when idle; Antigravity: Off or Notify).
//!
//! Nothing here polls, reads a login, makes a request or switches by itself: a caller hands each
//! watcher `check(now)`, and the watchers reach the outside only through [`sources`].

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
