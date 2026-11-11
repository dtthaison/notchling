//! E8c: token cost (Milestone 6), ported from `CostTracker.swift` and `CostSources.swift`. It only
//! reads transcripts, so it is not behind a switch.
//!
//! - [`prices`]: what a Claude message costs (at the catalog's prices), model keys and names.
//! - [`other`]: Codex's rollouts and Antigravity's transcripts, and [`other::OtherPrices`].
//! - [`tracker`]: [`tracker::Tracker`], the scan over the roots, with a [`cache`] of its own
//!   (never the Swift app's), deduped by `message.id|requestId`, with per-model token totals.
//! - [`summary`]: 7-day, 30-day and all-time [`summary::Summary`]s, overall and per account.
//! - [`service`]: [`service::CostService`], the tracker for the daemon: a request starts a scan on a
//!   thread and answers with what the last scan published.
//! - [`account_log`]: which account was active when, read from the account store's log.
//!
//! Channels: Claude Code (`~/.claude/projects`), Desktop (the Claude desktop app's
//! `local-agent-mode-sessions` and `claude-code-sessions`, the account taken from the path),
//! Codex (`~/.codex/sessions/**/rollout-*.jsonl`) and Antigravity CLI
//! (`brain/*/.system_generated/logs/transcript.jsonl`).

pub mod account_log;
pub mod cache;
pub mod other;
pub mod paths;
pub mod prices;
pub mod service;
pub mod summary;
pub mod tracker;

#[cfg(test)]
mod sources_tests;
#[cfg(test)]
mod tracker_tests;

pub use service::{CostService, Snapshot};
pub use summary::{Summary, Window};
pub use tracker::{Root, Tracker};
