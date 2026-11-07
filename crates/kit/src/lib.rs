//! The design kit: tokens and parts shared by every view.

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "kit"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "kit");
    }
}
