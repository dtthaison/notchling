use super::{Capability, Support, Tool};

pub trait Provider: Send + Sync {
    fn tool(&self) -> Tool;
    fn support(&self, c: Capability) -> Support;
    fn wired(&self) -> &'static [Capability];
}
