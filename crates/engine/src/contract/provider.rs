use super::channel::{self, Channel};
use super::subagents::{Subagents, UNPORTED};
use super::{Capability, Support, Tool};

pub trait Provider: Send + Sync {
    fn tool(&self) -> Tool;
    fn support(&self, c: Capability) -> Support;
    fn wired(&self) -> &'static [Capability];

    fn channels(&self) -> &'static [&'static dyn Channel] {
        channel::of(self.tool())
    }

    fn subagents(&self) -> &dyn Subagents {
        &UNPORTED
    }
}
