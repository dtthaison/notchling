use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSBitmapImageRep, NSControlStateValueOn, NSEventModifierFlags, NSImage, NSMenu, NSMenuDelegate,
    NSMenuItem, NSSquareStatusItemLength, NSStatusBar, NSStatusItem, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{
    NSData, NSNotification, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
    NSTimer,
};

use super::tray_draw::drawn_image;
use crate::menu::{
    a11y, pulse_action, tray_menu, Command, Dots, Entry, MenuState, PulseAction, TrayGlyph,
    TrayLook,
};
use crate::tray_glyph::{frames_of, pulse_frame_at, pulse_step_seconds};
use crate::tray_sync::TrayPort;

const GLYPH_SIZE: NSSize = NSSize::new(22.0, 16.0);
const OUTLINE: [&[u8]; 2] = [
    include_bytes!("../../../../assets/brand/menubarTemplate.png"),
    include_bytes!("../../../../assets/brand/menubarTemplate@2x.png"),
];
const FILLED: [&[u8]; 2] = [
    include_bytes!("../../../../assets/brand/menubarTemplateFilled.png"),
    include_bytes!("../../../../assets/brand/menubarTemplateFilled@2x.png"),
];

fn glyph_image(glyph: TrayGlyph, description: &str) -> Retained<NSImage> {
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
    image.setAccessibilityDescription(Some(&NSString::from_str(description)));
    image
}

const APPLY_DELAY: f64 = 0.1;

static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

fn read_reduce_motion() {
    REDUCE_MOTION.store(
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
        Ordering::Relaxed,
    );
}

fn observe_reduce_motion() -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    read_reduce_motion();
    let block = RcBlock::new(|_: NonNull<NSNotification>| read_reduce_motion());
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is AppKit's own constant, no queue is given so the block runs on the
    // posting thread, and it only stores one atomic.
    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            None,
            None,
            &block,
        )
    }
}

pub struct TargetIvars {
    core: RefCell<Weak<Core>>,
    on_command: Box<dyn Fn(Command)>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and MenuTarget doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotchlingMenuTarget"]
    #[ivars = TargetIvars]
    pub struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    unsafe impl NSMenuDelegate for MenuTarget {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &NSMenu) {
            if let Some(core) = self.ivars().core.borrow().upgrade() {
                core.open.set(true);
            }
        }

        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, _menu: &NSMenu) {
            if let Some(core) = self.ivars().core.borrow().upgrade() {
                core.open.set(false);
                core.apply_pending_later();
            }
        }
    }

    impl MenuTarget {
        #[unsafe(method(fire:))]
        fn fire(&self, sender: &NSMenuItem) {
            let command = usize::try_from(sender.tag()).ok().and_then(|i| {
                let core = self.ivars().core.borrow().upgrade()?;
                let commands = core.commands.borrow();
                commands.get(i).cloned()
            });
            if let Some(command) = command {
                (self.ivars().on_command)(command);
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, on_command: Box<dyn Fn(Command)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars {
            core: RefCell::new(Weak::new()),
            on_command,
        });
        // SAFETY: NSObject's plain init on an allocated instance with its ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct Shape {
    glyph: TrayGlyph,
    dots: Dots,
    pulse: bool,
}

struct Core {
    item: Retained<NSStatusItem>,
    target: RefCell<Option<Retained<MenuTarget>>>,
    mtm: MainThreadMarker,
    open: Cell<bool>,
    pending: RefCell<Option<MenuState>>,
    commands: RefCell<Vec<Command>>,
    cache: RefCell<Option<(Shape, Vec<Retained<NSImage>>)>>,
    last: Cell<Option<TrayLook>>,
    pulse: RefCell<Option<Retained<NSTimer>>>,
    frames: RefCell<Vec<Retained<NSImage>>>,
    observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    step: Cell<usize>,
    me: RefCell<Weak<Core>>,
}

pub struct Tray {
    core: Rc<Core>,
}

impl Tray {
    pub fn install(
        mtm: MainThreadMarker,
        state: &MenuState,
        on_command: Box<dyn Fn(Command)>,
    ) -> Tray {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSSquareStatusItemLength);
        let target = MenuTarget::new(mtm, on_command);
        let core = Rc::new(Core {
            item,
            target: RefCell::new(None),
            mtm,
            open: Cell::new(false),
            pending: RefCell::new(None),
            commands: RefCell::new(Vec::new()),
            cache: RefCell::new(None),
            last: Cell::new(None),
            pulse: RefCell::new(None),
            frames: RefCell::new(Vec::new()),
            observer: RefCell::new(Some(observe_reduce_motion())),
            step: Cell::new(0),
            me: RefCell::new(Weak::new()),
        });
        *target.ivars().core.borrow_mut() = Rc::downgrade(&core);
        *core.me.borrow_mut() = Rc::downgrade(&core);
        *core.target.borrow_mut() = Some(target);
        let mut tray = Tray { core };
        tray.set_look(&crate::menu::tray_look(state, reduce_motion()));
        tray.set_menu(state);
        tray
    }
}

impl Tray {
    pub fn pulse_again(&self) {
        self.core.pulse_again();
    }
}

impl TrayPort for Tray {
    fn set_look(&mut self, look: &TrayLook) {
        self.core.set_look(look);
    }

    fn set_menu(&mut self, state: &MenuState) {
        self.core.set_menu(state);
    }
}

impl Core {
    fn set_look(&self, look: &TrayLook) {
        let running = self.pulse.borrow().is_some();
        let action = pulse_action(running, self.last.get().as_ref(), look);
        self.last.set(Some(*look));
        if action != PulseAction::Keep {
            self.stop_pulse();
        }
        let description = a11y(look);
        let images = self.images_for(look, &description);
        let shown = match action {
            PulseAction::Keep => pulse_frame_at(self.step.get()).unwrap_or(0),
            _ => 0,
        };
        if let Some(button) = self.item.button(self.mtm) {
            if let Some(image) = images.get(shown).or(images.first()) {
                button.setImage(Some(image));
            }
        }
        match action {
            PulseAction::Start => self.start_pulse(images),
            PulseAction::Keep => *self.frames.borrow_mut() = images,
            PulseAction::Stop => {}
        }
    }

    fn pulse_again(&self) {
        let Some(look) = self.last.get() else {
            return;
        };
        if !look.pulse || self.pulse.borrow().is_some() {
            return;
        }
        let images = self.images_for(&look, &a11y(&look));
        self.start_pulse(images);
    }

    fn images_for(&self, look: &TrayLook, description: &str) -> Vec<Retained<NSImage>> {
        let frames = frames_of(look);
        if frames.is_empty() {
            *self.cache.borrow_mut() = None;
            return vec![glyph_image(look.glyph, description)];
        }
        let shape = Shape {
            glyph: look.glyph,
            dots: look.dots,
            pulse: look.pulse,
        };
        let mut cache = self.cache.borrow_mut();
        match cache.as_ref() {
            Some((held, images)) if *held == shape => {
                for image in images {
                    image.setAccessibilityDescription(Some(&NSString::from_str(description)));
                }
                images.clone()
            }
            _ => {
                let images: Vec<Retained<NSImage>> = frames
                    .into_iter()
                    .map(|frame| drawn_image(frame, description))
                    .collect();
                *cache = Some((shape, images.clone()));
                images
            }
        }
    }

    fn stop_pulse(&self) {
        if let Some(timer) = self.pulse.borrow_mut().take() {
            timer.invalidate();
        }
    }

    fn start_pulse(&self, images: Vec<Retained<NSImage>>) {
        if images.len() < 2 {
            return;
        }
        self.step.set(0);
        *self.frames.borrow_mut() = images;
        let me = self.me.borrow().clone();
        let block = RcBlock::new(move |_: NonNull<NSTimer>| {
            if let Some(core) = me.upgrade() {
                core.tick();
            }
        });
        // SAFETY: the block only runs on the main run loop's thread, where every object it
        // touches lives; it is never sent anywhere else.
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_repeats_block(pulse_step_seconds(), true, &block)
        };
        // SAFETY: the timer is added to the main thread's run loop from the main thread.
        unsafe {
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
        *self.pulse.borrow_mut() = Some(timer);
    }

    fn tick(&self) {
        let step = self.step.get() + 1;
        self.step.set(step);
        let shown = pulse_frame_at(step);
        if shown.is_none() {
            self.stop_pulse();
        }
        let index = shown.unwrap_or(0);
        let frames = self.frames.borrow();
        if let (Some(button), Some(image)) = (self.item.button(self.mtm), frames.get(index)) {
            button.setImage(Some(image));
        }
    }

    fn set_menu(&self, state: &MenuState) {
        if self.open.get() {
            *self.pending.borrow_mut() = Some(state.clone());
            return;
        }
        *self.pending.borrow_mut() = None;
        self.build_menu(state);
    }

    fn apply_pending_later(&self) {
        let me = self.me.borrow().clone();
        let block = RcBlock::new(move |_: NonNull<NSTimer>| {
            if let Some(core) = me.upgrade() {
                if !core.open.get() {
                    let pending = core.pending.borrow_mut().take();
                    if let Some(state) = pending {
                        core.build_menu(&state);
                    }
                }
            }
        });
        // SAFETY: the block only runs on the main run loop's thread, and touches only this core.
        let timer =
            unsafe { NSTimer::timerWithTimeInterval_repeats_block(APPLY_DELAY, false, &block) };
        // SAFETY: the timer is added to the main thread's run loop from the main thread.
        unsafe {
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
    }

    fn build_menu(&self, state: &MenuState) {
        let mut commands = Vec::new();
        let menu = self.build(&tray_menu(state), &mut commands);
        if let Some(target) = self.target.borrow().as_ref() {
            menu.setDelegate(Some(ProtocolObject::from_ref(&**target)));
        }
        *self.commands.borrow_mut() = commands;
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
                    if let Some(target) = self.target.borrow().as_ref() {
                        let object: &AnyObject = target.as_ref();
                        // SAFETY: the core keeps the target alive for as long as the menu is.
                        unsafe { item.setTarget(Some(object)) };
                    }
                    item.setTag(commands.len() as isize);
                    commands.push(command.clone());
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

impl Drop for Core {
    fn drop(&mut self) {
        self.stop_pulse();
        if let Some(observer) = self.observer.borrow_mut().take() {
            let object: &AnyObject = observer.as_ref();
            // SAFETY: the observer was added to this same center and is removed once.
            unsafe {
                NSWorkspace::sharedWorkspace()
                    .notificationCenter()
                    .removeObserver(object);
            }
        }
    }
}
