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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeAct {
    Cancel,
    HideParts,
    Pass,
}

pub fn escape_act(parts_open: bool, embedded: bool, key: &str) -> EscapeAct {
    if key != "escape" {
        EscapeAct::Pass
    } else if !parts_open {
        EscapeAct::Cancel
    } else if embedded {
        EscapeAct::Pass
    } else {
        EscapeAct::HideParts
    }
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
    fn escape_hides_the_part_labels_before_it_cancels_a_form() {
        for embedded in [false, true] {
            assert_eq!(escape_act(false, embedded, "escape"), EscapeAct::Cancel);
            assert_eq!(escape_act(false, embedded, "enter"), EscapeAct::Pass);
            assert_eq!(escape_act(true, embedded, "enter"), EscapeAct::Pass);
        }
    }

    #[test]
    fn a_form_in_its_own_window_hides_the_labels_itself_and_one_in_chat_leaves_it_to_chat() {
        assert_eq!(escape_act(true, false, "escape"), EscapeAct::HideParts);
        assert_eq!(escape_act(true, true, "escape"), EscapeAct::Pass);
    }

    #[test]
    fn only_the_window_and_the_sheet_take_the_keyboard_on_their_own() {
        assert!(!Presence::Window.embedded());
        assert!(Presence::Card.embedded() && Presence::Sheet.embedded());
        assert!(Presence::Window.takes_focus() && Presence::Sheet.takes_focus());
        assert!(!Presence::Card.takes_focus());
    }
}
