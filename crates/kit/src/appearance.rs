//! Light or dark: the system setting every token resolves against.

/// The system appearance. Dark first (the kit looks like the notch); light is warm paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 2] = [Appearance::Light, Appearance::Dark];

    /// The name as a view name suffix (`kit-gallery-dark`).
    pub fn name(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    /// Parses `light` or `dark`.
    pub fn parse(text: &str) -> Option<Appearance> {
        Self::ALL.into_iter().find(|a| a.name() == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::parse(a.name()), Some(a));
        }
        assert_eq!(Appearance::parse("dim"), None);
    }
}
