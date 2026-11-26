use std::sync::atomic::{AtomicBool, Ordering};

static SHOWN: AtomicBool = AtomicBool::new(false);

pub fn next(shown: bool, warned: bool, denied: bool, notch_shown: bool) -> bool {
    denied && !notch_shown && (shown || warned)
}

pub fn update(warned: bool, denied: bool, notch_shown: bool) {
    let now = next(SHOWN.load(Ordering::Relaxed), warned, denied, notch_shown);
    SHOWN.store(now, Ordering::Relaxed);
}

pub fn shown() -> bool {
    SHOWN.load(Ordering::Relaxed)
}

#[cfg(test)]
#[path = "warning_tests.rs"]
mod tests;
