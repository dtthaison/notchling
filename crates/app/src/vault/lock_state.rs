use std::sync::atomic::{AtomicU8, Ordering};

use super::reveal::Epoch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Screen,
    Sleep,
    Display,
    Session,
}

impl Cause {
    const fn bit(self) -> u8 {
        match self {
            Cause::Screen => 1,
            Cause::Sleep => 2,
            Cause::Display => 4,
            Cause::Session => 8,
        }
    }
}

pub struct Lock {
    epoch: Epoch,
    held: AtomicU8,
}

impl Lock {
    pub const fn new() -> Lock {
        Lock {
            epoch: Epoch::new(),
            held: AtomicU8::new(0),
        }
    }

    pub fn enter(&self, cause: Cause) {
        self.held.fetch_or(cause.bit(), Ordering::SeqCst);
        self.epoch.bump();
    }

    pub fn leave(&self, cause: Cause) {
        self.held.fetch_and(!cause.bit(), Ordering::SeqCst);
    }

    pub fn is_locked(&self) -> bool {
        self.held.load(Ordering::SeqCst) != 0
    }

    pub fn epoch(&self) -> &Epoch {
        &self.epoch
    }
}

impl Default for Lock {
    fn default() -> Lock {
        Lock::new()
    }
}

pub fn reads_usernames(form_open: bool, locked: bool) -> bool {
    !form_open && !locked
}

#[cfg(test)]
mod tests {
    use super::super::reveal::Seen;
    use super::*;

    #[test]
    fn a_lock_a_sleep_or_a_sleeping_display_holds_until_each_of_them_has_ended() {
        let lock = Lock::new();
        assert!(!lock.is_locked());
        let mut seen = Seen::of(lock.epoch());
        lock.enter(Cause::Screen);
        assert!(lock.is_locked() && seen.moved(lock.epoch()));
        lock.enter(Cause::Display);
        lock.leave(Cause::Display);
        assert!(lock.is_locked(), "the screen is still locked");
        lock.leave(Cause::Screen);
        assert!(!lock.is_locked());
        assert!(seen.moved(lock.epoch()));
        lock.leave(Cause::Screen);
        assert!(!seen.moved(lock.epoch()), "ending a lock is not a new lock");
        lock.enter(Cause::Sleep);
        lock.enter(Cause::Session);
        lock.leave(Cause::Sleep);
        assert!(lock.is_locked());
        lock.leave(Cause::Session);
        assert!(!lock.is_locked());
    }

    #[test]
    fn usernames_are_read_again_only_with_no_form_open_and_no_lock() {
        assert!(reads_usernames(false, false));
        assert!(!reads_usernames(true, false));
        assert!(!reads_usernames(false, true));
        assert!(!reads_usernames(true, true));
    }
}
