use super::{Capability, Provider, Support, Tool};

pub mod support;

pub struct ClaudeProvider;

impl ClaudeProvider {
    pub const WIRED: &'static [Capability] = &[];
}

impl Provider for ClaudeProvider {
    fn tool(&self) -> Tool {
        Tool::Claude
    }

    fn support(&self, c: Capability) -> Support {
        support::support(c)
    }

    fn wired(&self) -> &'static [Capability] {
        ClaudeProvider::WIRED
    }
}
