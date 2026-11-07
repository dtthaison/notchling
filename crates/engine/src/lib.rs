//! The engine: transcripts, sessions and accounts, with no UI.

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
