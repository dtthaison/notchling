//! The engine: transcripts, sessions and accounts, with no UI.
//!
//! Milestone 1 fills `transcript` (E1), `sessions` (E2) and `settings` (E14).

pub mod reply;
pub mod search;
pub mod sessions;
pub mod settings;
pub mod transcript;

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
