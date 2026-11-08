//! Platform adapters: keep awake, sleep and wake, notifications, file watching, private files, folders.
//!
//! Milestone 1 fills these for E12.

pub mod awake;
pub mod files;
pub mod notify;
pub mod paths;

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "platform"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "platform");
    }
}
