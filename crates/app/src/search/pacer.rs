//! When the panel asks the daemon: a key press waits a moment for the next one, and only one
//! question is out at a time. The newest query always wins: while one is out, a newer one waits
//! for it to come back and then goes alone, so typing a long word costs a few calls, not one per
//! letter, and answers can't arrive out of order. No clock or task here (the view owns those), so
//! the rules are tested as plain state.

use std::time::Duration;

/// How long after the last key press the query is sent.
pub const DEBOUNCE: Duration = Duration::from_millis(120);

/// What to do when a key press's wait is over.
#[derive(Debug, PartialEq, Eq)]
pub enum Settled {
    /// Ask the daemon now; give the generation back to [`Pacer::finished`] with the answer.
    Ask(u64),
    /// A call is out: this query goes when it returns.
    Held,
    /// Another key press came after this one and owns the question.
    Stale,
}

/// What to do with an answer.
#[derive(Debug, PartialEq, Eq)]
pub struct Answered {
    /// Show it: no newer key press has come since the question was asked.
    pub apply: bool,
    /// The generation to ask next, now that the call is back, if a newer query was waiting.
    pub next: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Pacer {
    /// Bumped by every key press; the query it names is the one now in the box.
    generation: u64,
    /// A question is out.
    in_flight: bool,
    /// The generation whose wait ended while a call was out.
    held: Option<u64>,
}

impl Pacer {
    /// The query changed (or is asked again): the generation to settle after [`DEBOUNCE`].
    pub fn typed(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// The newest query's generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The wait after key press `generation` is over.
    pub fn settled(&mut self, generation: u64) -> Settled {
        if generation != self.generation {
            return Settled::Stale;
        }
        if self.in_flight {
            self.held = Some(generation);
            return Settled::Held;
        }
        self.in_flight = true;
        Settled::Ask(generation)
    }

    /// The call asked for `asked` is back.
    pub fn finished(&mut self, asked: u64) -> Answered {
        self.in_flight = false;
        let next = self.held.take().filter(|g| *g == self.generation);
        if next.is_some() {
            self.in_flight = true;
        }
        Answered {
            apply: asked == self.generation,
            next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_key_press_of_a_burst_is_the_one_asked() {
        let mut p = Pacer::default();
        let (a, b, c) = (p.typed(), p.typed(), p.typed());
        assert_eq!(p.settled(a), Settled::Stale);
        assert_eq!(p.settled(b), Settled::Stale);
        assert_eq!(p.settled(c), Settled::Ask(c));
    }

    #[test]
    fn a_newer_query_waits_for_the_call_in_flight_and_goes_alone() {
        let mut p = Pacer::default();
        let first = p.typed();
        assert_eq!(p.settled(first), Settled::Ask(first));
        // Typing on while the call is out: the wait ends, but nothing is sent.
        let second = p.typed();
        assert_eq!(p.settled(second), Settled::Held);
        // The first answer is old news; the held query goes out at once.
        let done = p.finished(first);
        assert_eq!(
            done,
            Answered {
                apply: false,
                next: Some(second)
            }
        );
        // And exactly one call is out again: a third query waits for it.
        let third = p.typed();
        assert_eq!(p.settled(third), Settled::Held);
        let done = p.finished(second);
        assert_eq!(
            done,
            Answered {
                apply: false,
                next: Some(third)
            }
        );
        let done = p.finished(third);
        assert_eq!(
            done,
            Answered {
                apply: true,
                next: None
            }
        );
        // Nothing is out any more: the next key press asks directly.
        let fourth = p.typed();
        assert_eq!(p.settled(fourth), Settled::Ask(fourth));
    }

    #[test]
    fn an_answer_to_the_newest_query_is_shown() {
        let mut p = Pacer::default();
        let g = p.typed();
        assert_eq!(p.settled(g), Settled::Ask(g));
        assert_eq!(
            p.finished(g),
            Answered {
                apply: true,
                next: None
            }
        );
    }

    #[test]
    fn a_query_typed_after_the_hold_is_not_asked_before_its_own_wait() {
        let mut p = Pacer::default();
        let first = p.typed();
        assert_eq!(p.settled(first), Settled::Ask(first));
        let second = p.typed();
        assert_eq!(p.settled(second), Settled::Held);
        // Another key press arrives before the first call returns: the held one is stale and
        // the newest one asks after its own wait.
        let third = p.typed();
        let done = p.finished(first);
        assert_eq!(
            done,
            Answered {
                apply: false,
                next: None
            }
        );
        assert_eq!(p.settled(third), Settled::Ask(third));
    }

    #[test]
    fn asking_again_for_the_same_text_keeps_one_call_out() {
        // The indexing retry takes a new generation like a key press.
        let mut p = Pacer::default();
        let g = p.typed();
        assert_eq!(p.settled(g), Settled::Ask(g));
        let retry = p.typed();
        assert_eq!(p.settled(retry), Settled::Held);
        assert_eq!(p.generation(), retry);
    }
}
