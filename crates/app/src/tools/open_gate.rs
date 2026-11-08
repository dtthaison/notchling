//! When Chat asks the daemon to open an Antigravity session (`agy.open`). The daemon holds one
//! `agy acp` per session and closes it after ten idle minutes, or when it dies, so "asked once"
//! is not enough: a row the daemon closed has to be asked for again the next time Chat shows it,
//! or its conversation and Stop and reply box go dead until Chat restarts. The gate remembers an
//! ask only while it can still be answered.
//!
//! - A session the daemon says is open needs no ask, and forgets the earlier one: when it closes
//!   later it is asked for again.
//! - An ask waits for its answer for [`HOLD_MS`]; a change to the session's conversation or state
//!   since (the `tools` topic's `closed` and `error` among them) or a failed `agy.open` ends the
//!   wait, so the next look asks again, though never sooner than [`RETRY_MS`] after the last ask.
//!   A session that can't start doesn't ask in a loop.
//! - Acting turning on or off forgets every ask: what was asked while it was off was refused,
//!   and a session opened before it went off may be gone.

use std::collections::HashMap;

/// How long an ask waits for its answer when nothing about the session has changed.
pub const HOLD_MS: u64 = 15_000;
/// The least time between two asks for one session.
pub const RETRY_MS: u64 = 3_000;

#[derive(Clone, Copy, Debug)]
struct Ask {
    at: u64,
    /// The session's change count when asked; a different count since means it was answered.
    changes: u64,
}

#[derive(Debug, Default)]
pub struct OpenGate {
    asked: HashMap<String, Ask>,
    acting: bool,
}

impl OpenGate {
    /// Whether to send `agy.open` for `id` now, and if so, counts it as asked. `is_open` is the
    /// daemon's word that it holds the session, `changes` its change count (`agy_changes`),
    /// `acting` whether acting is on, `now_ms` the clock.
    pub fn should_open(
        &mut self,
        id: &str,
        is_open: bool,
        acting: bool,
        changes: u64,
        now_ms: u64,
    ) -> bool {
        if acting != self.acting {
            self.acting = acting;
            self.asked.clear();
        }
        if is_open {
            self.asked.remove(id);
            return false;
        }
        if !acting {
            return false;
        }
        if let Some(ask) = self.asked.get(id) {
            let since = now_ms.saturating_sub(ask.at);
            let answered = ask.changes != changes;
            if since < RETRY_MS || (!answered && since < HOLD_MS) {
                return false;
            }
        }
        self.asked.insert(
            id.to_owned(),
            Ask {
                at: now_ms,
                changes,
            },
        );
        true
    }

    /// `agy.open` for `id` failed: it may be asked for again, after the retry floor.
    pub fn failed(&mut self, id: &str) {
        if let Some(ask) = self.asked.get_mut(id) {
            ask.changes = u64::MAX;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "agy:s1";

    fn gate() -> OpenGate {
        let mut g = OpenGate::default();
        // The first look with acting on.
        assert!(!g.should_open("agy:other", true, true, 0, 0));
        g
    }

    #[test]
    fn a_closed_session_is_asked_for_again() {
        let mut g = gate();
        assert!(
            g.should_open(ID, false, true, 0, 1_000),
            "never opened: ask"
        );
        assert!(!g.should_open(ID, false, true, 0, 2_000), "asked, waiting");
        // The daemon says it is open now.
        assert!(!g.should_open(ID, true, true, 1, 3_000));
        // Ten idle minutes later the daemon closed it (the change count moved).
        assert!(
            g.should_open(ID, false, true, 2, 700_000),
            "closed: ask again"
        );
        assert!(!g.should_open(ID, false, true, 2, 700_100), "and only once");
    }

    #[test]
    fn a_close_seen_before_the_open_is_still_asked_again() {
        // The open and the close both arrive between two looks: the gate never saw it open.
        let mut g = gate();
        assert!(g.should_open(ID, false, true, 0, 1_000));
        assert!(!g.should_open(ID, false, true, 0, 2_000));
        assert!(
            !g.should_open(ID, false, true, 2, 2_500),
            "changed, but not three seconds after the ask"
        );
        assert!(
            g.should_open(ID, false, true, 2, 4_100),
            "changed and past the floor"
        );
    }

    #[test]
    fn a_failed_open_is_tried_again_but_not_in_a_loop() {
        let mut g = gate();
        assert!(g.should_open(ID, false, true, 0, 1_000));
        g.failed(ID);
        assert!(
            !g.should_open(ID, false, true, 0, 1_500),
            "inside the floor"
        );
        assert!(g.should_open(ID, false, true, 0, 4_100), "after it");
        g.failed(ID);
        assert!(!g.should_open(ID, false, true, 0, 4_200));
    }

    #[test]
    fn an_ask_that_gets_no_answer_is_repeated_after_the_hold() {
        let mut g = gate();
        assert!(g.should_open(ID, false, true, 0, 1_000));
        assert!(!g.should_open(ID, false, true, 0, 1_000 + HOLD_MS - 1));
        assert!(g.should_open(ID, false, true, 0, 1_000 + HOLD_MS));
    }

    #[test]
    fn acting_off_asks_nothing_and_acting_on_asks_afresh() {
        let mut g = OpenGate::default();
        assert!(!g.should_open(ID, false, false, 0, 1_000), "acting is off");
        assert!(g.should_open(ID, false, true, 0, 2_000));
        assert!(!g.should_open(ID, false, true, 0, 2_100));
        // Off, then on again: what was asked before is forgotten.
        assert!(!g.should_open(ID, false, false, 0, 2_200));
        assert!(g.should_open(ID, false, true, 0, 2_300));
    }

    #[test]
    fn sessions_are_asked_for_separately() {
        let mut g = gate();
        assert!(g.should_open("agy:a", false, true, 0, 1_000));
        assert!(g.should_open("agy:b", false, true, 0, 1_000));
        assert!(!g.should_open("agy:a", false, true, 0, 1_100));
    }
}
