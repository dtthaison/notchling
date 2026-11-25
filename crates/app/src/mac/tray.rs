use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSBitmapImageRep, NSControlStateValueOn, NSEventModifierFlags, NSImage, NSMenu, NSMenuItem,
    NSSquareStatusItemLength, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{ns_string, NSData, NSSize, NSString};

use crate::menu::{tray_glyph, tray_menu, Command, Entry, MenuState, TrayGlyph};

const GLYPH_SIZE: NSSize = NSSize::new(22.0, 16.0);
const OUTLINE: [&[u8]; 2] = [
    include_bytes!("../../../../assets/brand/menubarTemplate.png"),
    include_bytes!("../../../../assets/brand/menubarTemplate@2x.png"),
];
const FILLED: [&[u8]; 2] = [
    include_bytes!("../../../../assets/brand/menubarTemplateFilled.png"),
    include_bytes!("../../../../assets/brand/menubarTemplateFilled@2x.png"),
];

fn glyph_image(glyph: TrayGlyph) -> Retained<NSImage> {
    let reps = match glyph {
        TrayGlyph::Outline => OUTLINE,
        TrayGlyph::Filled => FILLED,
    };
    let image = NSImage::initWithSize(NSImage::alloc(), GLYPH_SIZE);
    for png in reps {
        if let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png)) {
            rep.setSize(GLYPH_SIZE);
            image.addRepresentation(&rep);
        }
    }
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(ns_string!("Notchling")));
    image
}

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

pub struct Tray {
    item: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
    mtm: MainThreadMarker,
}

impl Tray {
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

    pub fn update(&self, state: &MenuState) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setImage(Some(&glyph_image(tray_glyph(state))));
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
