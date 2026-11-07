//! The local API the desktop app and the phone bridge talk to.

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "local-api"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "local-api");
    }
}
