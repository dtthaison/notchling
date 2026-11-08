//! The menu bar item (`NSStatusItem`) and its menu, built from `crate::menu`'s entries. Picking an
//! entry calls back with its `Command`; the shell decides what it means.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSControlStateValueOn, NSEventModifierFlags, NSImage, NSMenu, NSMenuItem,
    NSSquareStatusItemLength, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{ns_string, NSString};

use crate::menu::{tray_menu, tray_symbol, Command, Entry, MenuState};

/// What the target holds: the commands by menu item tag, and whom to tell.
pub struct TargetIvars {
    commands: RefCell<Vec<Command>>,
    on_command: Box<dyn Fn(Command)>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and MenuTarget doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotchlingMenuTarget"]
    #[ivars = TargetIvars]
    pub struct MenuTarget;

    impl MenuTarget {
        /// Every action item points here; its tag says which command it is.
        #[unsafe(method(fire:))]
        fn fire(&self, sender: &NSMenuItem) {
            let command = usize::try_from(sender.tag())
                .ok()
                .and_then(|i| self.ivars().commands.borrow().get(i).copied());
            if let Some(command) = command {
                (self.ivars().on_command)(command);
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, on_command: Box<dyn Fn(Command)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars {
            commands: RefCell::new(Vec::new()),
            on_command,
        });
        // SAFETY: NSObject's plain init on an allocated instance with its ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The installed item. Dropping it removes the item from the menu bar.
pub struct Tray {
    item: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
    mtm: MainThreadMarker,
}

impl Tray {
    /// Puts the item in the menu bar with the menu for `state`.
    pub fn install(
        mtm: MainThreadMarker,
        state: &MenuState,
        on_command: Box<dyn Fn(Command)>,
    ) -> Tray {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSSquareStatusItemLength);
        let tray = Tray {
            item,
            target: MenuTarget::new(mtm, on_command),
            mtm,
        };
        tray.update(state);
        tray
    }

    /// Rebuilds the menu and the icon for a new state (a mode picked, an account switched).
    pub fn update(&self, state: &MenuState) {
        if let Some(button) = self.item.button(self.mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(tray_symbol(state)),
                Some(ns_string!("Notchling")),
            );
            button.setImage(image.as_deref());
        }
        let mut commands = self.target.ivars().commands.borrow_mut();
        commands.clear();
        let menu = self.build(&tray_menu(state), &mut commands);
        self.item.setMenu(Some(&menu));
    }

    fn build(&self, entries: &[Entry], commands: &mut Vec<Command>) -> Retained<NSMenu> {
        let menu = NSMenu::new(self.mtm);
        for entry in entries {
            match entry {
                Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(self.mtm)),
                Entry::Header(title) | Entry::Disabled(title) => {
                    let item = self.item_titled(title, false, "");
                    item.setEnabled(false);
                    menu.addItem(&item);
                }
                Entry::Action {
                    title,
                    command,
                    key,
                    checked,
                } => {
                    let key = key.map(|k| k.to_string()).unwrap_or_default();
                    let item = self.item_titled(title, true, &key);
                    // SAFETY: the target is kept alive by this Tray for as long as the menu is.
                    unsafe { item.setTarget(Some(&self.target)) };
                    item.setTag(commands.len() as isize);
                    commands.push(*command);
                    if *checked {
                        item.setState(NSControlStateValueOn);
                    }
                    menu.addItem(&item);
                }
                Entry::Submenu { title, items } => {
                    let item = self.item_titled(title, false, "");
                    item.setSubmenu(Some(&self.build(items, commands)));
                    menu.addItem(&item);
                }
            }
        }
        menu
    }

    fn item_titled(&self, title: &str, fires: bool, key: &str) -> Retained<NSMenuItem> {
        let action = fires.then(|| sel!(fire:));
        // SAFETY: a fresh NSMenuItem from AppKit's designated initializer; the selector exists on
        // MenuTarget, which every firing item gets as its target.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                self.mtm.alloc(),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        };
        if !key.is_empty() {
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        }
        item
    }
}
