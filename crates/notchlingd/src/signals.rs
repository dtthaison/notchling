//! SIGTERM and SIGINT ask the daemon to stop: the handler only sets a flag, which the main loop
//! reads, so the socket is removed by ordinary code, not inside the handler.

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    // Only an atomic store: async-signal-safe.
    STOP.store(true, Ordering::SeqCst);
}

/// Installs the handlers.
#[cfg(unix)]
pub fn install() {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

/// Windows: the daemon can't serve there yet, so there is nothing to stop.
#[cfg(not(unix))]
pub fn install() {}

/// Whether a stop was asked for.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}
