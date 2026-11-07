//! The desktop app: windows, views and wiring.

/// The crate's short name.
pub fn crate_name() -> &'static str {
    "app"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "app");
    }
}
