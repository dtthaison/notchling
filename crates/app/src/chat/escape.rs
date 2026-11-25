#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Open {
    pub parts: bool,
    pub menu: bool,
    pub popup: bool,
    pub subagent: bool,
    pub working: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    HideParts,
    CloseMenu,
    ClosePopup,
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
