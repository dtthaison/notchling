//! Spinners drawn and turned by Core Animation. A working session's spinner used to cost a frame
//! of the whole window ten times a second (3.8 ms each: GPUI draws the root and presents the whole
//! window). Here each slot the kit records (`notchling_kit::ui::SpinSlot`) is a `CAShapeLayer`
//! ring (Lucide's `loader-circle` arc, same size, stroke and ink) with a repeating rotation the
//! render server turns by itself: the app draws no frame to spin it.
//!
//! The layers are sublayers of the window view's own layer (GPUI's `CAMetalLayer`), above what
//! GPUI draws. [`CaSpinners::sync`] runs each time the window draws and only keeps the layers in
//! step with the slots (add, move, remove, in a transaction with actions off, keyed by the slot's
//! stable key so a layer that only moved keeps its running animation). Each ring sits in a
//! container layer clipped to the slot's clip rect, since a mask on the ring would turn with it.
//! Anything GPUI draws over the window (a sheet, a menu) would be under the layers: the kit's
//! layer takes the slots back (`SpinGate`) while one is open, and this sink then holds none.
//!
//! Coordinates: the slots are in window points, origin top-left. The window view is not flipped, so
//! a layer's y runs up: [`layer_y`] converts. The AppKit calls are main thread only, which every
//! GPUI draw is; the layers are removed when the sink is dropped (the window's layer goes).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::rc::{Rc, Weak};

use notchling_kit::ui::{spin_diff, spin_period, spin_phase, SpinSink, SpinSlot};
use notchling_kit::Color;
use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_app_kit::NSView;
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::native_spin_geometry::{
    arc, container_frame, placed, ring_centre, turn_to, Frame, Placed, BOX, RADIUS, STROKE,
};

// An opaque Core Graphics type, so `msg_send!` checks its pointer encoding.
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}
unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}
#[repr(C)]
struct CGPath {
    _private: [u8; 0],
}
unsafe impl RefEncode for CGPath {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGPath", &[]));
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPathCreateMutable() -> *mut CGPath;
    fn CGPathAddArc(
        path: *mut CGPath,
        transform: *const c_void,
        x: f64,
        y: f64,
        radius: f64,
        start: f64,
        end: f64,
        clockwise: bool,
    );
    fn CGColorCreateSRGB(r: f64, g: f64, b: f64, a: f64) -> *mut CGColor;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const c_void);
}

#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    fn CACurrentMediaTime() -> f64;
}

fn class(name: &CStr) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

fn new_layer(name: &CStr) -> Option<Retained<AnyObject>> {
    let cls = class(name)?;
    // SAFETY: `new` on a Core Animation layer class returns an owned, initialised layer.
    unsafe { msg_send![cls, new] }
}

/// A transaction with implicit animations off, so a layer that moves or changes ink doesn't glide.
fn without_actions<T>(f: impl FnOnce() -> T) -> T {
    let transaction = class(c"CATransaction");
    if let Some(t) = transaction {
        // SAFETY: class methods of CATransaction, balanced begin/commit on this thread.
        unsafe {
            let _: () = msg_send![t, begin];
            let _: () = msg_send![t, setDisableActions: Bool::YES];
        }
    }
    let out = f();
    if let Some(t) = transaction {
        // SAFETY: closes the transaction opened above.
        unsafe {
            let _: () = msg_send![t, commit];
        }
    }
    out
}

/// One spinner: the container (clipped, placed) and the ring turning inside it.
struct Entry {
    container: Retained<AnyObject>,
    ring: Retained<AnyObject>,
    /// The ring's size and ink when its path and colour were made.
    made: Option<(f32, Color)>,
}

impl Entry {
    fn new(flipped: bool) -> Option<Entry> {
        let container = new_layer(c"CALayer")?;
        let ring = new_layer(c"CAShapeLayer")?;
        // SAFETY: plain property setters and `addSublayer:` on layers we own.
        unsafe {
            let _: () = msg_send![&*container, setMasksToBounds: Bool::YES];
            // Above anything else a sublayer of the view's layer could be.
            let _: () = msg_send![&*container, setZPosition: 1000.0f64];
            let _: () = msg_send![&*container, addSublayer: &*ring];
            let no_color: *mut CGColor = std::ptr::null_mut();
            let _: () = msg_send![&*ring, setFillColor: no_color];
            let round = NSString::from_str("round");
            let _: () = msg_send![&*ring, setLineCap: &*round];
        }
        let entry = Entry {
            container,
            ring,
            made: None,
        };
        entry.start_turning(flipped);
        Some(entry)
    }

    /// The repeating rotation, begun so every ring is at the same angle at any moment (the
    /// phase is the kit's clock, as the GPUI spinners share).
    fn start_turning(&self, flipped: bool) {
        let Some(cls) = class(c"CABasicAnimation") else {
            return;
        };
        let key = NSString::from_str("transform.rotation.z");
        let from = NSNumber::new_f64(0.0);
        let to = NSNumber::new_f64(turn_to(flipped));
        // SAFETY: builds an animation and adds it to our ring; CACurrentMediaTime is a plain getter.
        unsafe {
            let anim: Option<Retained<AnyObject>> = msg_send![cls, animationWithKeyPath: &*key];
            let Some(anim) = anim else { return };
            let _: () = msg_send![&*anim, setFromValue: &*from];
            let _: () = msg_send![&*anim, setToValue: &*to];
            let _: () = msg_send![&*anim, setDuration: spin_period().as_secs_f64()];
            let _: () = msg_send![&*anim, setRepeatCount: f32::INFINITY];
            let _: () = msg_send![&*anim, setRemovedOnCompletion: Bool::NO];
            let begin = CACurrentMediaTime() - spin_phase().as_secs_f64();
            let _: () = msg_send![&*anim, setBeginTime: begin];
            let name = NSString::from_str("spin");
            let _: () = msg_send![&*self.ring, addAnimation: &*anim, forKey: &*name];
        }
    }

    /// Places and styles the layers for `p`. The animation is left alone.
    fn apply(&mut self, p: &Placed, frame: Frame) {
        let size = f64::from(p.ring.w);
        // SAFETY: property setters on layers we own; the path and colour are created here and
        // released after the layer has taken its own reference.
        unsafe {
            let _: () = msg_send![&*self.container, setFrame: container_frame(p.clip, frame)];
            let _: () = msg_send![&*self.ring, setContentsScale: f64::from(frame.scale)];
            let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size, size));
            let _: () = msg_send![&*self.ring, setBounds: bounds];
            let _: () = msg_send![&*self.ring, setPosition: ring_centre(p, frame)];
            if self.made != Some((p.ring.w, p.ink)) {
                let k = size / BOX;
                let line = STROKE * k;
                let path = CGPathCreateMutable();
                let (start, end, clockwise) = arc(frame.flipped);
                CGPathAddArc(
                    path,
                    std::ptr::null(),
                    size / 2.0,
                    size / 2.0,
                    RADIUS * k,
                    start,
                    end,
                    clockwise,
                );
                let [r, g, b, a] = p.ink.channels();
                let color =
                    CGColorCreateSRGB(f64::from(r), f64::from(g), f64::from(b), f64::from(a));
                let _: () = msg_send![&*self.ring, setPath: path];
                let _: () = msg_send![&*self.ring, setLineWidth: line];
                let _: () = msg_send![&*self.ring, setStrokeColor: color];
                CFRelease(path as *const c_void);
                CFRelease(color as *const c_void);
                self.made = Some((p.ring.w, p.ink));
            }
        }
    }
}

struct State {
    view: Option<Retained<NSView>>,
    /// The window view's layer, the layers' parent.
    host: Option<Retained<AnyObject>>,
    entries: HashMap<u64, Entry>,
    last: HashMap<u64, Placed>,
    frame: Option<Frame>,
}

/// The slots of a window as Core Animation layers.
pub struct CaSpinners {
    state: RefCell<State>,
}

thread_local! {
    /// The sink made last, for `--render`'s self-check (`probe`).
    static LATEST: RefCell<Weak<CaSpinners>> = const { RefCell::new(Weak::new()) };
}

impl CaSpinners {
    pub fn new() -> Rc<CaSpinners> {
        let sinks = Rc::new(CaSpinners {
            state: RefCell::new(State {
                view: None,
                host: None,
                entries: HashMap::new(),
                last: HashMap::new(),
                frame: None,
            }),
        });
        LATEST.with(|latest| *latest.borrow_mut() = Rc::downgrade(&sinks));
        sinks
    }

    /// Finds the window's view and its layer, once.
    fn attach(state: &mut State, window: &gpui::Window) -> bool {
        if state.host.is_some() {
            return true;
        }
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return false;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return false;
        };
        // SAFETY: GPUI hands out the pointer to its live NSView for this window.
        let Some(view) = (unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) })
        else {
            return false;
        };
        // SAFETY: `layer` is a plain getter; a layer-backed view has one.
        let host: Option<Retained<AnyObject>> = unsafe { msg_send![&*view, layer] };
        state.host = host;
        state.view = Some(view);
        state.host.is_some()
    }

    fn frame_of(state: &State, window: &gpui::Window) -> Option<Frame> {
        let view = state.view.as_ref()?;
        let host = state.host.as_ref()?;
        // SAFETY: plain getters on the view and its layer.
        let flipped: Bool = unsafe { msg_send![&**host, isGeometryFlipped] };
        Some(Frame {
            height: view.bounds().size.height as f32,
            scale: window.scale_factor(),
            flipped: flipped.as_bool(),
        })
    }

    /// What the layers say now, for `--render`'s check: each ring's place in the window (points,
    /// top-left origin) and its presentation layer's rotation, which the render server advances.
    pub fn probe(&self) -> Vec<Probe> {
        let state = self.state.borrow();
        let Some(frame) = state.frame else {
            return Vec::new();
        };
        let mut out: Vec<Probe> = state
            .entries
            .iter()
            .map(|(key, entry)| {
                let presentation: Option<Retained<AnyObject>> =
                    // SAFETY: plain getter.
                    unsafe { msg_send![&*entry.ring, presentationLayer] };
                let path = NSString::from_str("transform.rotation.z");
                let rotation = presentation.and_then(|layer| {
                    // SAFETY: key-value read of the layer's rotation, an NSNumber.
                    let value: Option<Retained<NSNumber>> =
                        unsafe { msg_send![&*layer, valueForKeyPath: &*path] };
                    value.map(|n| n.as_f64())
                });
                // SAFETY: plain getters.
                let (frame_in_host, ring_pos): (NSRect, NSPoint) = unsafe {
                    (
                        msg_send![&*entry.container, frame],
                        msg_send![&*entry.ring, position],
                    )
                };
                let y_up = if frame.flipped {
                    ring_pos.y
                } else {
                    frame_in_host.size.height - ring_pos.y
                };
                let top = if frame.flipped {
                    frame_in_host.origin.y
                } else {
                    f64::from(frame.height) - frame_in_host.origin.y - frame_in_host.size.height
                };
                Probe {
                    key: *key,
                    x: frame_in_host.origin.x + ring_pos.x,
                    y: top + y_up,
                    rotation,
                }
            })
            .collect();
        out.sort_by_key(|p| p.key);
        out
    }

    /// The number of rings alive.
    pub fn count(&self) -> usize {
        self.state.borrow().entries.len()
    }
}

/// One ring as the layers hold it.
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    pub key: u64,
    /// The ring's centre in window points, origin top-left.
    pub x: f64,
    pub y: f64,
    /// `transform.rotation.z` of the presentation layer, radians (`None` before it is shown).
    pub rotation: Option<f64>,
}

/// The most recent sink's rings, from the thread that made it.
pub fn probe() -> Vec<Probe> {
    LATEST.with(|latest| {
        latest
            .borrow()
            .upgrade()
            .map(|s| s.probe())
            .unwrap_or_default()
    })
}

impl SpinSink for CaSpinners {
    fn sync(&self, window: &gpui::Window, slots: &[SpinSlot]) {
        let mut state = self.state.borrow_mut();
        if slots.is_empty() && state.entries.is_empty() {
            return;
        }
        if !Self::attach(&mut state, window) {
            return;
        }
        let Some(frame) = Self::frame_of(&state, window) else {
            return;
        };
        // A resize or a scale change moves every layer, so every one is placed again.
        if state.frame != Some(frame) {
            state.last.clear();
            state.frame = Some(frame);
        }
        let next: Vec<(u64, Placed)> = slots
            .iter()
            .filter_map(|slot| placed(slot).map(|p| (slot.key, p)))
            .collect();
        let changes = spin_diff(&state.last, &next);
        if changes.added.is_empty() && changes.changed.is_empty() && changes.removed.is_empty() {
            return;
        }
        let State {
            host,
            entries,
            last,
            ..
        } = &mut *state;
        let Some(host) = host.as_ref() else { return };
        without_actions(|| {
            for key in &changes.removed {
                if let Some(entry) = entries.remove(key) {
                    // SAFETY: detaches a layer we added.
                    unsafe {
                        let _: () = msg_send![&*entry.container, removeFromSuperlayer];
                    }
                }
                last.remove(key);
            }
            for (key, p) in changes.added.iter().chain(changes.changed.iter()) {
                if !entries.contains_key(key) {
                    let Some(entry) = Entry::new(frame.flipped) else {
                        continue;
                    };
                    // SAFETY: adds our container to the view's layer; the layer retains it.
                    unsafe {
                        let _: () = msg_send![&**host, addSublayer: &*entry.container];
                    }
                    entries.insert(*key, entry);
                }
                if let Some(entry) = entries.get_mut(key) {
                    entry.apply(p, frame);
                }
                last.insert(*key, *p);
            }
        });
    }
}

impl Drop for CaSpinners {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        for (_, entry) in state.entries.drain() {
            // SAFETY: detaches a layer we added; this runs on the main thread, where the window
            // and its entities are dropped.
            unsafe {
                let _: () = msg_send![&*entry.container, removeFromSuperlayer];
            }
        }
    }
}
