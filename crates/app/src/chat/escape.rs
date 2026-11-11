//! What Esc does in Chat: it closes the topmost thing open, one press each, and only with nothing
//! open does it stop a working session's turn, as Esc does in the terminal.

/// What is open when Esc is pressed.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Open {
    /// The part names, drawn over everything.
    pub parts: bool,
    /// A menu or picker of the reply box (the + menu, a model picker).
    pub menu: bool,
    /// A sheet (Accounts, Settings, New Session…), the phone popover or the account menu.
    pub popup: bool,
    /// A subagent's transcript is shown in place of the conversation.
    pub subagent: bool,
    /// The selected session's turn runs.
    pub working: bool,
}

/// The one thing an Esc press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    HideParts,
    CloseMenu,
    ClosePopup,
    /// Back to the session's conversation.
    CloseSubagent,
    Stop,
    Nothing,
}

pub fn escape(open: Open) -> Escape {
    if open.parts {
        Escape::HideParts
    } else if open.menu {
        Escape::CloseMenu
    } else if open.popup {
        Escape::ClosePopup
    } else if open.subagent {
        Escape::CloseSubagent
    } else if open.working {
        Escape::Stop
    } else {
        Escape::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_closes_a_popup_and_never_stops_the_turn_behind_it() {
        let open = Open {
            popup: true,
            working: true,
            ..Open::default()
        };
        assert_eq!(escape(open), Escape::ClosePopup);
    }

    #[test]
    fn esc_closes_the_topmost_thing_first() {
        let all = Open {
            parts: true,
            menu: true,
            popup: true,
            subagent: true,
            working: true,
        };
        assert_eq!(escape(all), Escape::HideParts);
        assert_eq!(
            escape(Open {
                parts: false,
                ..all
            }),
            Escape::CloseMenu
        );
    }

    #[test]
    fn esc_in_a_subagent_goes_back_and_never_stops_the_turn_behind_it() {
        let open = Open {
            subagent: true,
            working: true,
            ..Open::default()
        };
        assert_eq!(escape(open), Escape::CloseSubagent);
        // A popup over it closes first.
        let over = Open {
            popup: true,
            ..open
        };
        assert_eq!(escape(over), Escape::ClosePopup);
    }

    #[test]
    fn with_nothing_open_esc_stops_a_working_turn_only() {
        let working = Open {
            working: true,
            ..Open::default()
        };
        assert_eq!(escape(working), Escape::Stop);
        assert_eq!(escape(Open::default()), Escape::Nothing);
    }
}
