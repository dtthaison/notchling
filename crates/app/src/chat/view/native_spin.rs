pub const ROOT_SPINNERS: &[&str] = &["spin-activity", "send-spin"];

pub const NATIVE_ENV: &str = "NOTCHLING_NATIVE_SPINNER";

pub fn enabled() -> bool {
    enabled_for(std::env::var(NATIVE_ENV).ok().as_deref())
}

fn enabled_for(value: Option<&str>) -> bool {
    value != Some("0")
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Covers {
    pub sheet: bool,
    pub menu: bool,
    pub composer_menu: bool,
    pub limit_card: bool,
    pub drop_hint: bool,
    pub part_names: bool,
    pub inbox: bool,
}

pub fn covered(c: Covers) -> bool {
    c.sheet || c.menu || c.composer_menu || c.limit_card || c.drop_hint || c.part_names || c.inbox
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_open_leaves_the_spinners_to_core_animation() {
        assert!(!covered(Covers::default()));
    }

    #[test]
    fn any_overlay_takes_them_back() {
        let each = [
            Covers {
                sheet: true,
                ..Covers::default()
            },
            Covers {
                menu: true,
                ..Covers::default()
            },
            Covers {
                composer_menu: true,
                ..Covers::default()
            },
            Covers {
                limit_card: true,
                ..Covers::default()
            },
            Covers {
                drop_hint: true,
                ..Covers::default()
            },
            Covers {
                part_names: true,
                ..Covers::default()
            },
            Covers {
                inbox: true,
                ..Covers::default()
            },
        ];
        for c in each {
            assert!(covered(c), "{c:?}");
        }
    }

    #[test]
    fn only_a_zero_turns_it_off() {
        assert!(enabled_for(None));
        assert!(enabled_for(Some("1")));
        assert!(!enabled_for(Some("0")));
    }
}
