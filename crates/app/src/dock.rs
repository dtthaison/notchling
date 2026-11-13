#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Regular,
    Accessory,
}

pub fn policy_for(titled_windows: usize) -> Policy {
    if titled_windows > 0 {
        Policy::Regular
    } else {
        Policy::Accessory
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainItem {
    Settings,
    Quit,
    Undo,
    Redo,
    Separator,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Close,
}

impl MainItem {
    pub fn title(self) -> &'static str {
        match self {
            MainItem::Settings => "Settings…",
            MainItem::Quit => "Quit Notchling",
            MainItem::Undo => "Undo",
            MainItem::Redo => "Redo",
            MainItem::Separator => "",
            MainItem::Cut => "Cut",
            MainItem::Copy => "Copy",
            MainItem::Paste => "Paste",
            MainItem::SelectAll => "Select All",
            MainItem::Minimize => "Minimize",
            MainItem::Close => "Close",
        }
    }

    pub fn key(self) -> Option<&'static str> {
        match self {
            MainItem::Settings => Some("cmd-,"),
            MainItem::Quit => Some("cmd-q"),
            MainItem::Undo => Some("cmd-z"),
            MainItem::Redo => Some("cmd-shift-z"),
            MainItem::Separator => None,
            MainItem::Cut => Some("cmd-x"),
            MainItem::Copy => Some("cmd-c"),
            MainItem::Paste => Some("cmd-v"),
            MainItem::SelectAll => Some("cmd-a"),
            MainItem::Minimize => Some("cmd-m"),
            MainItem::Close => Some("cmd-w"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainMenu {
    pub name: &'static str,
    pub items: Vec<MainItem>,
}

pub fn main_menu() -> Vec<MainMenu> {
    vec![
        MainMenu {
            name: "Notchling",
            items: vec![MainItem::Settings, MainItem::Separator, MainItem::Quit],
        },
        MainMenu {
            name: "Edit",
            items: vec![
                MainItem::Undo,
                MainItem::Redo,
                MainItem::Separator,
                MainItem::Cut,
                MainItem::Copy,
                MainItem::Paste,
                MainItem::SelectAll,
            ],
        },
        MainMenu {
            name: "Window",
            items: vec![MainItem::Minimize, MainItem::Close],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_icon_only_while_a_titled_window_is_open() {
        assert_eq!(policy_for(0), Policy::Accessory);
        assert_eq!(policy_for(1), Policy::Regular);
        assert_eq!(policy_for(3), Policy::Regular);
    }

    #[test]
    fn main_menu_has_no_hide_and_every_command_has_a_key() {
        let menus = main_menu();
        assert_eq!(
            menus.iter().map(|m| m.name).collect::<Vec<_>>(),
            ["Notchling", "Edit", "Window"]
        );
        for item in menus.iter().flat_map(|m| m.items.iter()) {
            assert!(!item.title().contains("Hide"));
            assert_eq!(
                item.key().is_some(),
                *item != MainItem::Separator,
                "{:?}",
                item
            );
        }
    }
}
