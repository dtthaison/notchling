use super::{Capability, Provider, Support, Tool};

pub mod support;

pub struct CodexProvider;

impl CodexProvider {
    pub const WIRED: &'static [Capability] = &[];
}

impl Provider for CodexProvider {
    fn tool(&self) -> Tool {
        Tool::Codex
    }

    fn support(&self, c: Capability) -> Support {
        support::support(c)
    }

    fn wired(&self) -> &'static [Capability] {
        CodexProvider::WIRED
    }
}
