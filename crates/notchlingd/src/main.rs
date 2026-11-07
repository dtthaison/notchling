//! `notchlingd`: prints its name and version, then exits. The real work arrives in later phases.

/// The line the binary prints: `<name> <version>`.
pub fn version_line() -> String {
    format!(
        "{} {}",
        env!("CARGO_PKG_NAME").trim_start_matches("notchling-"),
        env!("CARGO_PKG_VERSION")
    )
}

fn main() {
    println!("{}", version_line());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_is_name_then_version() {
        assert_eq!(version_line(), "notchlingd 0.1.0");
    }
}
