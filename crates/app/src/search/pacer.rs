use std::time::Duration;

pub const DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Debug, PartialEq, Eq)]
pub enum Settled {
    Ask(u64),
    Held,
    Stale,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Answered {
    pub apply: bool,
    pub next: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Pacer {
    generation: u64,
    in_flight: bool,
    held: Option<u64>,
}

impl Pacer {
    pub fn typed(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

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
        let second = p.typed();
        assert_eq!(p.settled(second), Settled::Held);
        let done = p.finished(first);
        assert_eq!(
            done,
            Answered {
                apply: false,
                next: Some(second)
            }
        );
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
        let mut p = Pacer::default();
        let g = p.typed();
        assert_eq!(p.settled(g), Settled::Ask(g));
        let retry = p.typed();
        assert_eq!(p.settled(retry), Settled::Held);
        assert_eq!(p.generation(), retry);
    }
}
