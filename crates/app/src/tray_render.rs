use crate::menu::{BlockingRow, MenuState};
use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 5] = [
    ViewSpec {
        name: "tray-menu",
        size: (660.0, 440.0),
        about: "the menu bar menu with two blocking items, the first one's submenu open (made-up data)",
    },
    ViewSpec {
        name: "tray-menu-dark",
        size: (660.0, 440.0),
        about: "the same menu in the dark appearance",
    },
    ViewSpec {
        name: "tray-menu-light",
        size: (660.0, 440.0),
        about: "the same menu in the light appearance",
    },
    ViewSpec {
        name: "tray-menu-idle",
        size: (460.0, 330.0),
        about: "the menu bar menu with nothing blocking (made-up data)",
    },
    ViewSpec {
        name: "tray-glyphs",
        size: (760.0, 560.0),
        about: "the menu bar glyph in its five states, on a dark and a light menu bar, at size and zoomed",
    },
];

fn row(id: &str, title: &str, who: &str, waited: &str, allow: bool) -> BlockingRow {
    BlockingRow {
        id: id.to_owned(),
        title: title.to_owned(),
        who: who.to_owned(),
        waited: waited.to_owned(),
        allow_from_menu: allow,
        deny_from_menu: true,
    }
}

pub fn blocked_state() -> MenuState {
    MenuState {
        account: Some("me@example.com".to_owned()),
        blocking: vec![
            row("one", "Turn On Machines", "Claude Code", "3m", true),
            row("two", "Use npm-publish", "Codex", "41s", false),
        ],
        working: true,
        ..MenuState::default()
    }
}

pub fn idle_state() -> MenuState {
    MenuState {
        account: Some("me@example.com".to_owned()),
        ..MenuState::default()
    }
}
