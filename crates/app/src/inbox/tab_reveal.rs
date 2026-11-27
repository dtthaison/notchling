pub(super) const MAX_TRIES: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reveal {
    pub index: usize,
    tries: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Apply,
    Retry,
    Drop,
}

impl Reveal {
    pub(super) fn new(index: usize) -> Reveal {
        Reveal { index, tries: 0 }
    }

    pub(super) fn step(&mut self, ready: bool) -> Step {
        if ready {
            return Step::Apply;
        }
        self.tries = self.tries.saturating_add(1);
        if self.tries > MAX_TRIES {
            Step::Drop
        } else {
            Step::Retry
        }
    }
}
