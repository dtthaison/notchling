use notchling_engine::reply::exec::Pty;
use notchling_engine::reply::keys::{ENTER, PASTE_START};
use notchling_engine::terminal::Shade;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) struct Guarded<'a> {
    pub(crate) inner: Box<dyn Pty + 'a>,
    pub(crate) guard: &'a dyn Fn() -> bool,
    pub(crate) refused: &'a AtomicBool,
    pub(crate) typed: &'a AtomicBool,
    pub(crate) entered: Option<&'a AtomicBool>,
    pub(crate) checked: bool,
}

impl Pty for Guarded<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if !self.checked {
            self.checked = true;
            if !(self.guard)() {
                self.refused.store(true, Ordering::SeqCst);
                return Err(std::io::Error::other("the session is not at rest"));
            }
        }
        if bytes.starts_with(PASTE_START.as_bytes()) {
            self.typed.store(true, Ordering::SeqCst);
        }
        let wrote = self.inner.write(bytes);
        if let (Some(entered), true) = (self.entered, bytes == ENTER.as_bytes()) {
            entered.store(true, Ordering::SeqCst);
        }
        wrote
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }

    fn wait_attached(&mut self, quiet: Duration, timeout: Duration, busy: Duration) -> bool {
        self.inner.wait_attached(quiet, timeout, busy)
    }

    fn screen(&mut self) -> Vec<String> {
        self.inner.screen()
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.inner.cursor()
    }

    fn shades(&mut self) -> Vec<Vec<Shade>> {
        self.inner.shades()
    }

    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration)
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}

#[cfg(test)]
#[path = "turn_gate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "turn_gate_shades_tests.rs"]
mod shades_tests;
