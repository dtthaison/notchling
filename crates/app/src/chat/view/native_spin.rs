//! When Chat's spinners are Core Animation's (`mac::native_spin`) and when GPUI draws them.
//! Layers sit above everything GPUI draws, so while something GPUI draws over the window could
//! cover a spinner (a sheet, a menu, a popover, the part names) the kit's layer paints them itself
//! again: simple and correct beats clever, and those moments are short.

/// The spinners `ChatView` lays out itself that are drawn by slot: the Activity Line's, which
/// shows for as long as a session works. The reply box's short "sending" spinner, and the
/// sheets', stay views of their own.
pub const ROOT_SPINNERS: &[&str] = &["spin-activity"];

/// The environment's switch: `0` draws Chat's spinners the old way (GPUI, ten frames a second).
pub const NATIVE_ENV: &str = "NOTCHLING_NATIVE_SPINNER";

/// Whether Core Animation draws Chat's spinners at all.
pub fn enabled() -> bool {
    enabled_for(std::env::var(NATIVE_ENV).ok().as_deref())
}

fn enabled_for(value: Option<&str>) -> bool {
    value != Some("0")
}

/// What is open over the window, from what `ChatView` knows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Covers {
    /// New Session or Settings.
    pub sheet: bool,
    /// The header's account menu, the sidebar row menu, the Phone panel.
    pub menu: bool,
    /// The reply box's + menu, a picker (Model, Effort, tool options) or the command list.
    pub composer_menu: bool,
    /// The usage ring's card, over the reply box.
    pub limit_card: bool,
    /// The drop hint over the column.
    pub drop_hint: bool,
    /// The ? button's part names.
    pub part_names: bool,
}

/// Whether anything is open that GPUI draws over a spinner.
pub fn covered(c: Covers) -> bool {
    c.sheet || c.menu || c.composer_menu || c.limit_card || c.drop_hint || c.part_names
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
