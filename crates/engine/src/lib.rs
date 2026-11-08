//! The engine: transcripts, sessions and accounts, with no UI.
//!
//! Milestone 1 fills `transcript` (E1), `sessions` (E2) and `settings` (E14); milestone 3 adds
//! `control` (E11), which plans what the app does to a session and launches it through a trait,
//! `terminal` (E3a) and `reply` (E3b), which plans what is typed into a session. Milestone 6 adds
//! `vault` (E9), the Secret Vault.

pub mod bridge;
pub mod control;
pub mod delegate;
pub mod pages;
pub mod reply;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod terminal;
pub mod tools;
pub mod transcript;
pub mod vault;
pub mod viz;

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "engine"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "engine");
    }
}
