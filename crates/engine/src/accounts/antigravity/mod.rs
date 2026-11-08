//! Antigravity accounts (E8b, `Antigravity.swift`): the login in its go-keyring Keychain item and
//! the mirror file, our saved copies, and limits from its local language server.
//!
//! - [`login`]: the login formats and whose they are (pure).
//! - [`quota`]: the language server's limits, through [`crate::accounts::procs::Procs`] and
//!   [`crate::providers::HttpClient`].
//! - [`store`]: [`AntigravityStore`], the provider.

pub mod login;
pub mod quota;
pub mod store;

#[cfg(test)]
mod store_tests;

pub use store::{AntigravityDeps, AntigravityPaths, AntigravityStore};
