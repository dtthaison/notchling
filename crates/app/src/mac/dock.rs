//! Applying the Dock policy and the main menu (`crate::dock` decides; this file does).

use gpui::{actions, App, KeyBinding, Menu, MenuItem, OsAction};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use crate::dock::{main_menu, MainItem, Policy};

actions!(
    notchling,
    [
        OpenSettings,
        Quit,
        Minimize,
        CloseWindow,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        SelectAll
    ]
);

/// Sets the activation policy. A no-op off the main thread, where AppKit must not be touched.
pub fn apply(policy: Policy) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let wanted = match policy {
        Policy::Regular => NSApplicationActivationPolicy::Regular,
        Policy::Accessory => NSApplicationActivationPolicy::Accessory,
    };
    if app.activationPolicy() != wanted {
        app.setActivationPolicy(wanted);
    }
}

/// The main menu for the policy: the three menus while regular, nothing while in the menu bar only.
pub fn set_main_menu(cx: &mut App, policy: Policy) {
    let menus = match policy {
        Policy::Regular => main_menu()
            .into_iter()
            .map(|menu| Menu {
                name: menu.name.into(),
                items: menu.items.into_iter().map(menu_item).collect(),
            })
            .collect(),
        Policy::Accessory => Vec::new(),
    };
    cx.set_menus(menus);
}

fn menu_item(item: MainItem) -> MenuItem {
    let title = item.title();
    match item {
        MainItem::Settings => MenuItem::action(title, OpenSettings),
        MainItem::Quit => MenuItem::action(title, Quit),
        MainItem::Undo => MenuItem::os_action(title, Undo, OsAction::Undo),
        MainItem::Redo => MenuItem::os_action(title, Redo, OsAction::Redo),
        MainItem::Separator => MenuItem::separator(),
        MainItem::Cut => MenuItem::os_action(title, Cut, OsAction::Cut),
        MainItem::Copy => MenuItem::os_action(title, Copy, OsAction::Copy),
        MainItem::Paste => MenuItem::os_action(title, Paste, OsAction::Paste),
        MainItem::SelectAll => MenuItem::os_action(title, SelectAll, OsAction::SelectAll),
        MainItem::Minimize => MenuItem::action(title, Minimize),
        MainItem::Close => MenuItem::action(title, CloseWindow),
    }
}

/// Quit, Minimize and Close, and the key equivalents the menu shows. The edit actions reach the
/// focused text view once there is one (U1).
pub fn register(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &OpenSettings, cx| super::shell::open_chat_settings(cx));
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, _| window.minimize_window())
                .ok();
        }
    });
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    });
    let bindings = main_menu()
        .into_iter()
        .flat_map(|menu| menu.items)
        .filter_map(|item| item.key().map(|key| binding(key, item)))
        .collect::<Vec<_>>();
    cx.bind_keys(bindings);
}

fn binding(key: &str, item: MainItem) -> KeyBinding {
    match item {
        MainItem::Settings => KeyBinding::new(key, OpenSettings, None),
        MainItem::Quit | MainItem::Separator => KeyBinding::new(key, Quit, None),
        MainItem::Undo => KeyBinding::new(key, Undo, None),
        MainItem::Redo => KeyBinding::new(key, Redo, None),
        MainItem::Cut => KeyBinding::new(key, Cut, None),
        MainItem::Copy => KeyBinding::new(key, Copy, None),
        MainItem::Paste => KeyBinding::new(key, Paste, None),
        MainItem::SelectAll => KeyBinding::new(key, SelectAll, None),
        MainItem::Minimize => KeyBinding::new(key, Minimize, None),
        MainItem::Close => KeyBinding::new(key, CloseWindow, None),
    }
}
