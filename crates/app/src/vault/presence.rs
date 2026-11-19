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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_window_and_the_sheet_take_the_keyboard_on_their_own() {
        assert!(!Presence::Window.embedded());
        assert!(Presence::Card.embedded() && Presence::Sheet.embedded());
        assert!(Presence::Window.takes_focus() && Presence::Sheet.takes_focus());
        assert!(!Presence::Card.takes_focus());
    }
}
