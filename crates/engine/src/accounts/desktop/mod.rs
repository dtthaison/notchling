//! Claude Desktop accounts (E8b, `ClaudeDesktop.swift`): the whole claude.ai cookie store captured
//! per account and restored to switch, only while Desktop is not running.
//!
//! - [`files`]: the Cookies store, `config.json` and the one-time backup.
//! - [`store`]: [`DesktopStore`], the provider.

pub mod files;
pub mod store;

#[cfg(test)]
mod store_tests;

pub use store::{ClaudeAccounts, ClaudeName, DesktopDeps, DesktopPaths, DesktopStore};
