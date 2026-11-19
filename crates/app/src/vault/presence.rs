#[derive(Clone, Copy, Debug)]
pub struct Done;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presence {
    #[default]
    Window,
    Card,
    Sheet,
}

impl Presence {
    pub fn embedded(self) -> bool {
        self != Presence::Window
    }

    pub fn takes_focus(self) -> bool {
        self != Presence::Card
    }
}

pub fn escape_denies(visible: bool, key: &str) -> bool {
    visible && key == "escape"
}

pub fn keeps_focus(visible: bool, presence: Presence) -> bool {
    visible || !presence.embedded()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_covered_prompt_neither_keeps_the_keyboard_nor_denies_on_escape() {
        assert!(escape_denies(true, "escape"));
        assert!(!escape_denies(false, "escape"));
        assert!(!escape_denies(true, "enter"));
        assert!(!keeps_focus(false, Presence::Card));
        assert!(!keeps_focus(false, Presence::Sheet));
        assert!(keeps_focus(true, Presence::Sheet));
        assert!(keeps_focus(false, Presence::Window));
    }

    #[test]
    fn only_the_window_and_the_sheet_take_the_keyboard_on_their_own() {
        assert!(!Presence::Window.embedded());
        assert!(Presence::Card.embedded() && Presence::Sheet.embedded());
        assert!(Presence::Window.takes_focus() && Presence::Sheet.takes_focus());
        assert!(!Presence::Card.takes_focus());
    }
}
