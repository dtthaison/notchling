#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Regular,
    Accessory,
}

pub const KEY: &str = "showInDock";

pub fn from_settings(settings: &notchling_engine::settings::Settings) -> bool {
    settings.get_bool(KEY).unwrap_or(true)
}

pub fn read(data_dir: &std::path::Path) -> bool {
    notchling_engine::settings::Settings::open(&data_dir.join("settings.json"))
        .map(|s| from_settings(&s))
        .unwrap_or(true)
}

pub fn changed_to(key: &str, value: &serde_json::Value) -> Option<bool> {
    (key == KEY).then_some(value)?.as_bool()
}

pub fn policy_for(titled_windows: usize, show_in_dock: bool) -> Policy {
    if show_in_dock || titled_windows > 0 {
        Policy::Regular
    } else {
        Policy::Accessory
    }
}

pub fn badge_for(blocking: usize, show_in_dock: bool) -> Option<u32> {
    if !show_in_dock || blocking == 0 {
        return None;
    }
    Some(u32::try_from(blocking).unwrap_or(u32::MAX))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Window,
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub policy: Policy,
    pub hold: bool,
    pub activate: bool,
}

pub fn plan(titled_windows: usize, show_in_dock: bool, popup_key: bool, cause: Cause) -> Plan {
    let policy = policy_for(titled_windows, show_in_dock);
    Plan {
        policy,
        hold: policy == Policy::Accessory && popup_key,
        activate: cause == Cause::Window && titled_windows > 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainItem {
    About,
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
            MainItem::About => "About Notchling",
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
            MainItem::About => None,
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
    main_menu_for(true)
}

pub fn main_menu_for(app_shell: bool) -> Vec<MainMenu> {
    let app_items = if app_shell {
        vec![
            MainItem::About,
            MainItem::Separator,
            MainItem::Settings,
            MainItem::Separator,
            MainItem::Quit,
        ]
    } else {
        vec![MainItem::Quit]
    };
    vec![
        MainMenu {
            name: "Notchling",
            items: app_items,
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
    fn the_badge_counts_blocking_items_and_only_while_the_icon_shows() {
        assert_eq!(badge_for(0, true), None);
        assert_eq!(badge_for(1, true), Some(1));
        assert_eq!(badge_for(7, true), Some(7));
        assert_eq!(badge_for(7, false), None);
    }

    #[test]
    fn without_show_in_dock_the_icon_is_only_while_a_titled_window_is_open() {
        assert_eq!(policy_for(0, false), Policy::Accessory);
        assert_eq!(policy_for(1, false), Policy::Regular);
        assert_eq!(policy_for(3, false), Policy::Regular);
    }

    #[test]
    fn with_show_in_dock_the_icon_is_there_with_no_window() {
        assert_eq!(policy_for(0, true), Policy::Regular);
        assert_eq!(policy_for(2, true), Policy::Regular);
    }

    #[test]
    fn show_in_dock_is_on_when_nothing_was_saved() {
        use crate::testutil::Scratch;
        use notchling_engine::settings::{spec, Fallback, Settings};
        use serde_json::json;
        assert_eq!(spec(KEY).expect("listed").default, Fallback::Bool(true));
        let scratch = Scratch::new("show-in-dock");
        assert!(read(scratch.path()));
        let mut settings = Settings::open(&scratch.path().join("settings.json")).expect("open");
        assert!(from_settings(&settings));
        assert!(read(scratch.path()));
        settings.set(KEY, json!(false)).expect("set");
        assert!(!from_settings(&settings));
        assert!(!read(scratch.path()));
        settings.set(KEY, json!(true)).expect("set");
        assert!(from_settings(&settings));
        assert!(read(scratch.path()));
    }

    #[test]
    fn a_setting_change_never_activates_the_app() {
        for titled in [0, 1, 3] {
            for show in [false, true] {
                for popup in [false, true] {
                    assert!(!plan(titled, show, popup, Cause::Quiet).activate);
                }
            }
        }
    }

    #[test]
    fn a_window_change_activates_the_app_only_while_a_titled_window_is_open() {
        assert!(plan(1, false, false, Cause::Window).activate);
        assert!(plan(2, true, false, Cause::Window).activate);
        assert!(!plan(0, false, false, Cause::Window).activate);
        assert!(!plan(0, true, false, Cause::Window).activate);
    }

    #[test]
    fn the_icon_is_held_while_a_popup_has_the_keys_and_nothing_wants_it() {
        let held = plan(0, false, true, Cause::Quiet);
        assert_eq!(held.policy, Policy::Accessory);
        assert!(held.hold);
        assert!(plan(0, false, true, Cause::Window).hold);
    }

    #[test]
    fn nothing_is_held_when_the_icon_stays_or_no_popup_has_the_keys() {
        assert!(!plan(0, true, true, Cause::Quiet).hold);
        assert!(!plan(1, false, true, Cause::Window).hold);
        assert!(!plan(0, false, false, Cause::Quiet).hold);
        assert!(!plan(0, false, false, Cause::Window).hold);
    }

    #[test]
    fn only_the_show_in_dock_key_with_a_bool_is_a_change() {
        use serde_json::json;
        assert_eq!(changed_to("showInDock", &json!(false)), Some(false));
        assert_eq!(changed_to("showInDock", &json!(true)), Some(true));
        assert_eq!(changed_to("showInDock", &json!("off")), None);
        assert_eq!(changed_to("appearance", &json!(false)), None);
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
                !matches!(item, MainItem::Separator | MainItem::About),
                "{:?}",
                item
            );
        }
    }

    #[test]
    fn app_menu_starts_with_about_then_a_separator() {
        let app = &main_menu()[0];
        assert_eq!(app.name, "Notchling");
        assert_eq!(
            app.items,
            [
                MainItem::About,
                MainItem::Separator,
                MainItem::Settings,
                MainItem::Separator,
                MainItem::Quit
            ]
        );
        assert_eq!(MainItem::About.title(), "About Notchling");
    }

    #[test]
    fn without_the_app_shell_the_app_menu_has_only_quit() {
        let menus = main_menu_for(false);
        assert_eq!(menus[0].items, [MainItem::Quit]);
        assert_eq!(menus[1..], main_menu()[1..]);
    }
}
