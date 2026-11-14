use super::{Capability, Provider, Support, Tool};

pub mod support;

pub struct AntigravityProvider;

impl AntigravityProvider {
    pub const WIRED: &'static [Capability] = &[];
}

impl Provider for AntigravityProvider {
    fn tool(&self) -> Tool {
        Tool::Antigravity
    }

    fn support(&self, c: Capability) -> Support {
        support::support(c)
    }

    fn wired(&self) -> &'static [Capability] {
        AntigravityProvider::WIRED
    }
}
