#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 2] = [Appearance::Light, Appearance::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

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
