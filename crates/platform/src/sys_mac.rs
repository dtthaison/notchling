//! The macOS system calls this crate makes by hand: a few CoreFoundation and IOKit functions.
//! No Objective-C is needed for power assertions or sleep and wake notices.

use std::ffi::{c_char, c_void, CString};

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFRunLoopRef = *mut c_void;

pub const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub const IO_RETURN_SUCCESS: i32 = 0;
/// `kIOPMAssertionLevelOn`.
pub const ASSERTION_LEVEL_ON: u32 = 255;

pub type PowerCallback = extern "C" fn(*mut c_void, u32, u32, *mut c_void);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub fn CFStringCreateWithCString(
        alloc: *const c_void,
        text: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFRelease(value: CFTypeRef);
    pub fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after: u8) -> i32;
    pub fn CFRunLoopStop(run_loop: CFRunLoopRef);
    pub fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: *mut c_void, mode: CFStringRef);
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    pub fn IOPMAssertionCreateWithName(
        kind: CFStringRef,
        level: u32,
        name: CFStringRef,
        id: *mut u32,
    ) -> i32;
    pub fn IOPMAssertionRelease(id: u32) -> i32;
    pub fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut *mut c_void,
        callback: PowerCallback,
        notifier: *mut u32,
    ) -> u32;
    pub fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
    pub fn IOAllowPowerChange(root_port: u32, notification_id: isize) -> i32;
    pub fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> *mut c_void;
    pub fn IONotificationPortDestroy(port: *mut c_void);
    pub fn IOServiceClose(connection: u32) -> i32;
}

/// An owned CoreFoundation string, released on drop.
pub struct CfString(CFStringRef);

impl CfString {
    /// `None` when the text has an interior NUL or CoreFoundation can't make the string.
    pub fn new(text: &str) -> Option<CfString> {
        let c = CString::new(text).ok()?;
        // SAFETY: `c` is a valid NUL-terminated string for the call; a null result is handled.
        let raw = unsafe {
            CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), CF_STRING_ENCODING_UTF8)
        };
        (!raw.is_null()).then_some(CfString(raw))
    }

    pub fn as_raw(&self) -> CFStringRef {
        self.0
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a Create function and is released exactly once.
        unsafe { CFRelease(self.0) };
    }
}

/// libproc's `PROC_ALL_PIDS`; the `libc` crate doesn't name it.
const PROC_ALL_PIDS: u32 = 1;

/// Every process and the file it runs, from the kernel's process table (`proc_listpids`,
/// `proc_pidpath`). Unlike the system's running-application list, which a process without a main
/// run loop never hears updates to, this is read fresh on every call. `None` when the table can't
/// be listed; a process whose path can't be read (it ended since the listing) is left out.
pub fn process_paths() -> Option<Vec<(i32, String)>> {
    const PID: usize = std::mem::size_of::<libc::pid_t>();
    // SAFETY: a null buffer of size 0 asks only for the size of the list in bytes.
    let size = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    let count = usize::try_from(size).ok().filter(|n| *n > 0)? / PID;
    // Room for processes that start between the two calls.
    let mut pids: Vec<libc::pid_t> = vec![0; count + 64];
    let bytes = libc::c_int::try_from(pids.len() * PID).ok()?;
    // SAFETY: `pids` is writable for `bytes` bytes.
    let got = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(usize::try_from(got).ok().filter(|n| *n > 0)? / PID);
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let mut table = Vec::with_capacity(pids.len());
    for pid in pids.into_iter().filter(|p| *p > 0) {
        // SAFETY: `buf` is writable for its full length.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if let Some(len) = usize::try_from(n).ok().filter(|n| *n > 0) {
            table.push((pid, String::from_utf8_lossy(&buf[..len]).into_owned()));
        }
    }
    Some(table)
}
