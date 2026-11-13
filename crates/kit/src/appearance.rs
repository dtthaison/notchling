use std::sync::atomic::{AtomicU8, Ordering};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preference {
    #[default]
    System,
    Light,
    Dark,
}

impl Preference {
    pub const ALL: [Preference; 3] = [Preference::System, Preference::Light, Preference::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Preference::System => "system",
            Preference::Light => "light",
            Preference::Dark => "dark",
        }
    }

    pub fn parse(text: &str) -> Option<Preference> {
        Self::ALL.into_iter().find(|p| p.name() == text)
    }

    pub fn forced(self) -> Option<Appearance> {
        match self {
            Preference::System => None,
            Preference::Light => Some(Appearance::Light),
            Preference::Dark => Some(Appearance::Dark),
        }
    }

    pub fn resolve(self, system: Appearance) -> Appearance {
        self.forced().unwrap_or(system)
    }
}

const SYSTEM: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;

static CHOSEN: AtomicU8 = AtomicU8::new(SYSTEM);

pub fn set_preference(preference: Preference) {
    let code = match preference {
        Preference::System => SYSTEM,
        Preference::Light => LIGHT,
        Preference::Dark => DARK,
    };
    CHOSEN.store(code, Ordering::Relaxed);
}

pub fn preference() -> Preference {
    match CHOSEN.load(Ordering::Relaxed) {
        LIGHT => Preference::Light,
        DARK => Preference::Dark,
        _ => Preference::System,
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

    #[test]
    fn a_preference_resolves_to_its_own_look_or_the_systems() {
        for system in Appearance::ALL {
            assert_eq!(Preference::System.resolve(system), system);
            assert_eq!(Preference::Light.resolve(system), Appearance::Light);
            assert_eq!(Preference::Dark.resolve(system), Appearance::Dark);
        }
    }

    #[test]
    fn preference_names_round_trip() {
        for p in Preference::ALL {
            assert_eq!(Preference::parse(p.name()), Some(p));
        }
        assert_eq!(Preference::parse("auto"), None);
        assert_eq!(Preference::default(), Preference::System);
    }

    #[test]
    fn the_stored_preference_reads_back() {
        for p in Preference::ALL {
            set_preference(p);
            assert_eq!(preference(), p);
        }
        set_preference(Preference::System);
    }
}
