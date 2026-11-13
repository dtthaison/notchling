use std::cell::Cell;

use gpui::Pixels;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSResponder, NSView};
use objc2_core_graphics::{CGEvent, CGEventField, CGEventType, CGMouseButton, CGScrollEventUnit};
use objc2_foundation::{NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub struct Injector {
    view: Retained<NSView>,
    window_number: isize,
    height: f64,
    scroll_origin: Cell<Option<f64>>,
}

impl Injector {
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
            scroll_origin: Cell::new(None),
        })
    }

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

    pub fn moved(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::MouseMoved, x, y, 0) {
            NSResponder::mouseMoved(&self.view, &event);
        }
    }

    pub fn press(&self, x: Pixels, y: Pixels) {
        if let Some(event) = self.mouse(NSEventType::LeftMouseDown, x, y, 1) {
            NSResponder::mouseDown(&self.view, &event);
        }
    }

    pub fn right_click(&self, x: Pixels, y: Pixels) {
        for kind in [CGEventType::RightMouseDown, CGEventType::RightMouseUp] {
            let wanted = self.at(x, y);
            let Some(cg) = CGEvent::new_mouse_event(
                None,
                kind,
                NSPoint::new(wanted.x, 0.0),
                CGMouseButton::Right,
            ) else {
                return;
            };
            CGEvent::set_integer_value_field(Some(&cg), CGEventField::MouseEventClickState, 1);
            let origin = self.screen_origin();
            CGEvent::set_location(Some(&cg), NSPoint::new(wanted.x, origin - wanted.y));
            let Some(event) = NSEvent::eventWithCGEvent(&cg) else {
                return;
            };
            if kind == CGEventType::RightMouseDown {
                NSResponder::rightMouseDown(&self.view, &event);
            } else {
                NSResponder::rightMouseUp(&self.view, &event);
            }
        }
    }

    fn screen_origin(&self) -> f64 {
        if let Some(origin) = self.scroll_origin.get() {
            return origin;
        }
        let probe = CGEvent::new_mouse_event(
            None,
            CGEventType::MouseMoved,
            NSPoint::new(0.0, 0.0),
            CGMouseButton::Left,
        )
        .and_then(|cg| NSEvent::eventWithCGEvent(&cg));
        let origin = probe.map_or(0.0, |event| event.locationInWindow().y);
        self.scroll_origin.set(Some(origin));
        origin
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

    pub fn scroll(&mut self, x: Pixels, y: Pixels, dy: f32) {
        let wanted = self.at(x, y);
        let Some(cg) =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, dy as i32, 0, 0)
        else {
            return;
        };
        CGEvent::set_integer_value_field(Some(&cg), CGEventField::ScrollWheelEventIsContinuous, 1);
        let place = |cg: &CGEvent, screen_y: f64| {
            CGEvent::set_location(Some(cg), NSPoint::new(wanted.x, screen_y));
        };
        let origin = match self.scroll_origin.get() {
            Some(origin) => origin,
            None => {
                place(&cg, 0.0);
                let Some(probe) = NSEvent::eventWithCGEvent(&cg) else {
                    return;
                };
                let origin = probe.locationInWindow().y;
                self.scroll_origin.set(Some(origin));
                origin
            }
        };
        place(&cg, origin - wanted.y);
        if let Some(event) = NSEvent::eventWithCGEvent(&cg) {
            NSResponder::scrollWheel(&self.view, &event);
        }
    }
}

pub struct FrameDriver {
    view: Retained<NSView>,
}

impl FrameDriver {
    pub fn frame(&self) {
        // SAFETY: GPUI's view class implements `displayLayer:` (it ignores its argument); both
        // calls run on the main thread, outside any borrow of the app.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*self.view, layer];
            let _: () = msg_send![&*self.view, displayLayer: layer];
        }
    }
}
