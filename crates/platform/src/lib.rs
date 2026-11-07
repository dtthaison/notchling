//! Platform adapters: keychain, processes, notifications.

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
