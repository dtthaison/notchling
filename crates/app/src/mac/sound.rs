use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::NSSound;
use objc2_foundation::{NSBundle, NSString};

pub const FILE: &str = "stopped";
pub const EXTENSION: &str = "aiff";

thread_local! {
    static PLAYING: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
}

fn bundled() -> Option<Retained<NSSound>> {
    let path = NSBundle::mainBundle().pathForResource_ofType(
        Some(&NSString::from_str(FILE)),
        Some(&NSString::from_str(EXTENSION)),
    )?;
    NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &path, true)
}

pub fn play() -> bool {
    let Some(sound) = bundled() else {
        return false;
    };
    let started = sound.play();
    PLAYING.with(|slot| *slot.borrow_mut() = Some(sound));
    started
}
