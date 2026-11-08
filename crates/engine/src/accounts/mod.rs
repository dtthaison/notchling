//! Accounts (E8): the logins Notchling saves and switches between. This part (E8b) is Codex,
//! Antigravity and Claude Desktop; Claude Code's accounts and lanes are E8a's.
//!
//! Every effect on a real login goes through a trait that has a fake:
//! [`generic_password::Keychain`] (`/usr/bin/security`), [`procs::Procs`] and [`procs::Apps`]
//! (processes and apps), [`crate::providers::HttpClient`] (limits), and a folder passed in. Tests
//! never touch the Keychain, a real login, the network or `~/.codex`, `~/.gemini` or Claude
//! Desktop's folder.
//!
//! Built and switched off: the daemon calls into these only while its `accounts` switch is on.
//! Never renews a token: refresh tokens rotate, and a session still holding the old one would be
//! signed out.

pub mod antigravity;
pub mod book;
pub mod codex;
pub mod desktop;
pub mod generic_password;
pub mod jwt;
pub mod procs;
