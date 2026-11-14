use notchling_engine::transcript::subagents::Subagent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingChild {
    id: String,
    loads_left: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Open(String),
    Waiting,
    GaveUp,
}

impl PendingChild {
    pub const LOADS: u8 = 3;

    pub fn new(id: &str) -> PendingChild {
        PendingChild {
            id: id.to_owned(),
            loads_left: Self::LOADS,
        }
    }

    pub fn after_load(&mut self, listed: &[Subagent]) -> Step {
        if listed.iter().any(|a| a.id == self.id) {
            return Step::Open(self.id.clone());
        }
        self.loads_left = self.loads_left.saturating_sub(1);
        if self.loads_left == 0 {
            Step::GaveUp
        } else {
            Step::Waiting
        }
    }
}

#[cfg(test)]
#[path = "pending_child_tests.rs"]
mod tests;
