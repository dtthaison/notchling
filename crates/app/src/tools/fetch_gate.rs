//! How Chat reads an Antigravity row's conversation from the daemon (`agy.conversation`): at most
//! one read in flight per row, and nothing older than what is on screen ever replaces it. A
//! streaming turn changes the conversation many times a second; asking once per change put a
//! read behind every event, and the answers could land out of order and draw the conversation
//! going backwards.
//!
//! A change while a read is out waits for it; when it returns, one more read goes, which covers
//! every change that came meanwhile. An answer is shown unless a newer one already was. (An
//! answer that is older than the newest change but newer than the screen is still shown: the
//! read behind it follows at once, and holding it back would starve the screen under a stream
//! faster than the daemon answers.)

use std::collections::HashMap;

/// What to do with a change.
#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    /// Read now; hand the number back with the answer.
    Start(u64),
    /// A read is out: this change is covered by the one that follows it.
    Wait,
}

/// What to do with an answer.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    /// Show it.
    pub apply: bool,
    /// Read again now, for changes that came while this read was out.
    pub next: Option<u64>,
}

#[derive(Default)]
struct Row {
    /// The newest read asked for (a change that may be waiting).
    requested: u64,
    /// The newest answer shown.
    shown: u64,
    in_flight: bool,
}

#[derive(Default)]
pub struct FetchGate {
    counter: u64,
    rows: HashMap<String, Row>,
}

impl FetchGate {
    /// The row's conversation changed (or was never read).
    pub fn request(&mut self, id: &str) -> Ask {
        self.counter += 1;
        let row = self.rows.entry(id.to_owned()).or_default();
        row.requested = self.counter;
        if row.in_flight {
            return Ask::Wait;
        }
        row.in_flight = true;
        Ask::Start(self.counter)
    }

    /// The read numbered `number` for the row is back.
    pub fn finished(&mut self, id: &str, number: u64) -> Done {
        let Some(row) = self.rows.get_mut(id) else {
            return Done {
                apply: false,
                next: None,
            };
        };
        row.in_flight = false;
        let apply = number > row.shown;
        if apply {
            row.shown = number;
        }
        let next = (row.requested > number).then_some(row.requested);
        row.in_flight = next.is_some();
        Done { apply, next }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_during_a_read_waits_and_one_more_read_covers_all_of_them() {
        let mut g = FetchGate::default();
        let first = match g.request("agy:a") {
            Ask::Start(n) => n,
            other => panic!("{other:?}"),
        };
        assert_eq!(g.request("agy:a"), Ask::Wait);
        assert_eq!(g.request("agy:a"), Ask::Wait);
        let done = g.finished("agy:a", first);
        assert!(done.apply, "newer than the screen");
        let next = done.next.expect("the changes that waited");
        assert!(next > first);
        // That read is out: another change waits again.
        assert_eq!(g.request("agy:a"), Ask::Wait);
        let done = g.finished("agy:a", next);
        assert!(done.apply);
        assert!(done.next.is_some(), "and the change that came during it");
    }

    #[test]
    fn nothing_is_asked_when_nothing_changed_meanwhile() {
        let mut g = FetchGate::default();
        let Ask::Start(n) = g.request("agy:a") else {
            panic!("a read");
        };
        assert_eq!(
            g.finished("agy:a", n),
            Done {
                apply: true,
                next: None
            }
        );
        // Idle: the next change reads at once.
        assert!(matches!(g.request("agy:a"), Ask::Start(_)));
    }

    #[test]
    fn an_older_answer_never_replaces_a_newer_one() {
        let mut g = FetchGate::default();
        let Ask::Start(first) = g.request("agy:a") else {
            panic!("a read");
        };
        // The first read came back late, after a newer read had been shown (a restarted view).
        g.rows.get_mut("agy:a").expect("a row").shown = first + 5;
        assert!(!g.finished("agy:a", first).apply);
    }

    #[test]
    fn rows_are_read_separately() {
        let mut g = FetchGate::default();
        assert!(matches!(g.request("agy:a"), Ask::Start(_)));
        assert!(
            matches!(g.request("agy:b"), Ask::Start(_)),
            "another row's read is its own"
        );
        assert_eq!(g.request("agy:a"), Ask::Wait);
        assert_eq!(g.request("agy:b"), Ask::Wait);
    }

    #[test]
    fn an_answer_nobody_asked_for_is_dropped() {
        let mut g = FetchGate::default();
        assert_eq!(
            g.finished("agy:x", 3),
            Done {
                apply: false,
                next: None
            }
        );
    }
}
