#[derive(Default)]
pub(super) struct Toggle {
    on: Option<bool>,
    asked: u64,
    cleared: u64,
}

impl Toggle {
    #[cfg(test)]
    pub(super) fn is(&self) -> Option<bool> {
        self.on
    }

    pub(super) fn off(&self) -> bool {
        self.on == Some(false)
    }

    pub(super) fn turn(&mut self, on: bool) -> Option<u64> {
        let before = self.on;
        self.on = Some(on);
        if on {
            return (self.cleared < self.asked).then_some(self.asked);
        }
        if before == Some(false) && self.cleared == self.asked {
            return None;
        }
        self.asked += 1;
        Some(self.asked)
    }

    pub(super) fn owed(&mut self, ticket: u64) -> Option<u64> {
        if !self.off() {
            self.settle(ticket);
            return None;
        }
        (self.cleared < ticket).then_some(self.asked)
    }

    pub(super) fn settle(&mut self, upto: u64) {
        self.cleared = self.cleared.max(upto);
    }
}
