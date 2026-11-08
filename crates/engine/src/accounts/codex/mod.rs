//! Codex accounts (E8b, `Codex.swift`, CLI 0.166): the login in `<CODEX_HOME>/auth.json`, saved
//! copies in the Keychain, the usage endpoint and the switch.
//!
//! - [`login`]: reading a login and the usage answer (pure).
//! - [`live`]: writing `auth.json` under a flock on `auth.lock`.
//! - [`store`]: [`CodexStore`], the provider.

pub mod live;
pub mod login;
pub mod store;

#[cfg(test)]
mod store_tests;
#[cfg(test)]
pub(crate) mod testkit;

pub use store::{CodexDeps, CodexPaths, CodexStore};
