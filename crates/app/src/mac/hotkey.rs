//! ⌘⇧P from anywhere, through Carbon's hot key API (the Swift `GlobalHotKey`). A registered hot
//! key needs no Accessibility permission and sees no other key press: macOS delivers only that
//! combination, as a Carbon event on the main run loop. The other systems have no implementation
//! yet (`search::hotkey::NoHotKey`).
//!
//! Carbon has no `objc2` crate, so the five functions are declared here and the framework is
//! linked by name. Each press reaches the handler registered for its id; the handlers live in a
//! thread-local, since Carbon calls back on the main thread and GPUI's contexts aren't `Send`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

use objc2::MainThreadMarker;

use crate::search::hotkey::{HotKey, Registration};

type OSStatus = i32;
type EventHotKeyRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventHandlerUPP = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

/// Carbon's `EventHotKeyID`: who registered the key and which one it is.
#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

/// Carbon's `EventTypeSpec`: the class and kind of event a handler takes.
#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    /// `ItemCount` and `ByteCount` are `unsigned long`: `usize` on every Mac this builds for.
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerUPP,
        num_types: usize,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        wanted: u32,
        out_type: *mut u32,
        size: usize,
        out_size: *mut usize,
        out_data: *mut c_void,
    ) -> OSStatus;
}

/// A four-character code as Carbon reads it.
const fn four(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const K_EVENT_CLASS_KEYBOARD: u32 = four(b"keyb");
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
const K_EVENT_PARAM_DIRECT_OBJECT: u32 = four(b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = four(b"hkid");
/// Our hot keys' signature, so another library's in this process are left alone.
const SIGNATURE: u32 = four(b"NTCL");
const K_VK_ANSI_P: u32 = 0x23;
const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const NO_ERR: OSStatus = 0;

thread_local! {
    /// What to call for each registered id, on the main thread.
    static HANDLERS: RefCell<HashMap<u32, Rc<dyn Fn()>>> = RefCell::new(HashMap::new());
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
}

/// Carbon's callback for a press: reads which key it was and calls its handler.
extern "C" fn on_hot_key(_: EventHandlerCallRef, event: EventRef, _: *mut c_void) -> OSStatus {
    let mut id = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    // SAFETY: a hot key event's direct object is an EventHotKeyID; Carbon writes exactly that
    // many bytes into `id`, which lives for the call.
    let status = unsafe {
        GetEventParameter(
            event,
            K_EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            ptr::null_mut(),
            (&mut id as *mut EventHotKeyID).cast(),
        )
    };
    if status == NO_ERR && id.signature == SIGNATURE {
        // Cloned out of the map first: a handler may register or drop a key itself.
        let handler = HANDLERS.with(|h| h.borrow().get(&id.id).cloned());
        if let Some(handler) = handler {
            handler();
        }
    }
    NO_ERR
}

/// Installs the one Carbon handler for hot key presses, the first time.
fn install_handler() -> Result<(), String> {
    if INSTALLED.with(Cell::get) {
        return Ok(());
    }
    let spec = EventTypeSpec {
        event_class: K_EVENT_CLASS_KEYBOARD,
        event_kind: K_EVENT_HOT_KEY_PRESSED,
    };
    // SAFETY: the application target is always valid in a running app; the spec outlives the
    // call (Carbon copies it); no user data and no handler reference are wanted.
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            on_hot_key,
            1,
            &spec,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status != NO_ERR {
        return Err(format!(
            "Carbon refused the hot key handler (status {status})"
        ));
    }
    INSTALLED.with(|i| i.set(true));
    Ok(())
}

/// A system-wide key over Carbon.
pub struct CarbonHotKey {
    key_code: u32,
    modifiers: u32,
    label: &'static str,
}

impl CarbonHotKey {
    /// The search panel's key.
    pub fn command_shift_p() -> CarbonHotKey {
        CarbonHotKey {
            key_code: K_VK_ANSI_P,
            modifiers: CMD_KEY | SHIFT_KEY,
            label: "⌘⇧P",
        }
    }
}

impl HotKey for CarbonHotKey {
    fn label(&self) -> &'static str {
        self.label
    }

    /// Registers the key. `Err` when another app already holds it, or off the main thread.
    fn register(&self, on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String> {
        MainThreadMarker::new().ok_or("a hot key is registered on the main thread")?;
        install_handler()?;
        let id = NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1).max(1));
            id
        });
        let mut hot_key: EventHotKeyRef = ptr::null_mut();
        // SAFETY: plain values in, and Carbon writes the new key's reference into `hot_key`.
        let status = unsafe {
            RegisterEventHotKey(
                self.key_code,
                self.modifiers,
                EventHotKeyID {
                    signature: SIGNATURE,
                    id,
                },
                GetApplicationEventTarget(),
                0,
                &mut hot_key,
            )
        };
        if status != NO_ERR || hot_key.is_null() {
            return Err(format!(
                "another app holds {} (Carbon status {status})",
                self.label
            ));
        }
        HANDLERS.with(|h| h.borrow_mut().insert(id, Rc::from(on_press)));
        Ok(Box::new(CarbonRegistration { id, hot_key }))
    }
}

/// A registered key; dropping it unregisters.
struct CarbonRegistration {
    id: u32,
    hot_key: EventHotKeyRef,
}

impl Registration for CarbonRegistration {}

impl Drop for CarbonRegistration {
    fn drop(&mut self) {
        // SAFETY: the reference came from RegisterEventHotKey and is unregistered once.
        unsafe { UnregisterEventHotKey(self.hot_key) };
        HANDLERS.with(|h| h.borrow_mut().remove(&self.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_carbons() {
        // Four-character codes read big-endian, as Carbon's headers spell them.
        assert_eq!(K_EVENT_CLASS_KEYBOARD, 0x6B65_7962);
        assert_eq!(TYPE_EVENT_HOT_KEY_ID, 0x686B_6964);
        assert_eq!(K_EVENT_PARAM_DIRECT_OBJECT, 0x2D2D_2D2D);
        assert_eq!(SIGNATURE, 0x4E54_434C);
        let key = CarbonHotKey::command_shift_p();
        assert_eq!(key.label(), "⌘⇧P");
        assert_eq!((key.key_code, key.modifiers), (0x23, 0x300));
        assert_eq!(std::mem::size_of::<EventHotKeyID>(), 8);
        // Nothing is registered here: that needs the running app and takes the real shortcut.
    }
}
