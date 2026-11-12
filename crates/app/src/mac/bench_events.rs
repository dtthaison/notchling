//! AppKit events for `--render chat`'s bench (`bench`). GPUI's `Window::dispatch_event` returns a
//! type GPUI keeps private, so a crate outside it can't call it; the bench hands the window's
//! view the events AppKit would: the same path a real mouse or trackpad takes, ending in GPUI's
//! `handle_view_event`. Nothing here moves the real pointer or posts a system event.

use gpui::Pixels;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSResponder, NSView};
use objc2_core_graphics::{CGEvent, CGEventField, CGScrollEventUnit};
use objc2_foundation::{NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Sends events to one window's content view.
pub struct Injector {
    view: Retained<NSView>,
    window_number: isize,
    /// The view's height in points: AppKit's y runs up from the bottom, GPUI's down from the top.
    height: f64,
    /// What a scroll event's location has to add, found once (see [`Injector::scroll`]).
    scroll_origin: Option<f64>,
}

impl Injector {
    /// The injector for a GPUI window, or `None` when it has no AppKit view.
    pub fn new(window: &gpui::Window) -> Option<Injector> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: GPUI hands out the pointer to its live NSView for this window.
        let view = unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr())? };
        let window_number = view.window().map_or(0, |w| w.windowNumber());
        let height = view.bounds().size.height;
        Some(Injector {
            view,
            window_number,
            height,
            scroll_origin: None,
        })
    }

    /// A handle that draws the window by hand (see [`FrameDriver`]).
    pub fn frame_driver(&self) -> FrameDriver {
        FrameDriver {
            view: self.view.clone(),
        }
    }

    fn at(&self, x: Pixels, y: Pixels) -> NSPoint {
        NSPoint::new(
            f64::from(f32::from(x)),
            self.height - f64::from(f32::from(y)),
        )
    }

    fn mouse(
        &self,
        kind: NSEventType,
        x: Pixels,
        y: Pixels,
        clicks: isize,
    ) -> Option<Retained<NSEvent>> {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            self.at(x, y),
            NSEventModifierFlags::empty(),
            0.0,
            self.window_number,
            None,
            0,
            clicks,
            1.0,
        )
    }

    /// The pointer moved to a point of the window.
    pub fn moved(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::MouseMoved, x, y, 0) {
            NSResponder::mouseMoved(&self.view, &event);
        }
    }

    /// The left button went down, or the pointer moved with it held (`drag`), or it came up.
    pub fn press(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::LeftMouseDown, x, y, 1) {
            NSResponder::mouseDown(&self.view, &event);
        }
    }

    pub fn drag(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::LeftMouseDragged, x, y, 1) {
            NSResponder::mouseDragged(&self.view, &event);
        }
    }

    pub fn release(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::LeftMouseUp, x, y, 1) {
            NSResponder::mouseUp(&self.view, &event);
        }
    }

    /// ⌘ and a key, offered to the window's view as AppKit offers a key equivalent (the first
    /// thing it does with a ⌘ key): whether the view took it. `code` is the key's virtual code.
    pub fn command_key(&self, letter: &str, code: u16) -> bool {
        let chars = NSString::from_str(letter);
        let Some(event) = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::Command,
            0.0,
            self.window_number,
            None,
            &chars,
            &chars,
            false,
            code,
        ) else {
            return false;
        };
        self.view.performKeyEquivalent(&event)
    }

    /// A trackpad scroll of `dy` points at a point of the window; positive moves the content down
    /// (towards what is above it). AppKit builds a scroll event only from a Core Graphics one,
    /// and an event with no window reports its location on the screen, from the bottom left: the
    /// first call finds what that adds to the window's own coordinates.
    pub fn scroll(&mut self, x: Pixels, y: Pixels, dy: f32) {
        let wanted = self.at(x, y);
        let Some(cg) =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, dy as i32, 0, 0)
        else {
            return;
        };
        // Pixel deltas count as a trackpad's only when the event says it is continuous.
        CGEvent::set_integer_value_field(Some(&cg), CGEventField::ScrollWheelEventIsContinuous, 1);
        let place = |cg: &CGEvent, screen_y: f64| {
            CGEvent::set_location(Some(cg), NSPoint::new(wanted.x, screen_y));
        };
        let origin = match self.scroll_origin {
            Some(origin) => origin,
            None => {
                // At screen y 0 (the top), AppKit's y is the screen's height.
                place(&cg, 0.0);
                let Some(probe) = NSEvent::eventWithCGEvent(&cg) else {
                    return;
                };
                let origin = probe.locationInWindow().y;
                self.scroll_origin = Some(origin);
                origin
            }
        };
        place(&cg, origin - wanted.y);
        if let Some(event) = NSEvent::eventWithCGEvent(&cg) {
            NSResponder::scrollWheel(&self.view, &event);
        }
    }
}

/// Draws the window the way AppKit does when a layer needs display, for a bench that has to run
/// while no display link ticks (the Mac's screen is locked or its display asleep). GPUI's
/// `displayLayer:` is its own first-frame path: it runs the window's frame callback, which draws
/// only when the window is dirty and presents, so a frame costs what a display link's would.
pub struct FrameDriver {
    view: Retained<NSView>,
}

impl FrameDriver {
    /// One frame, if the window has anything to draw.
    pub fn frame(&self) {
        // SAFETY: GPUI's view class implements `displayLayer:` (it ignores its argument); both
        // calls run on the main thread, outside any borrow of the app.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*self.view, layer];
            let _: () = msg_send![&*self.view, displayLayer: layer];
        }
    }
}
