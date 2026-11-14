use super::subagents::Subagents;
use super::{Capability, Provider, Support, Tool};

pub mod subagents;
pub mod support;

#[cfg(test)]
mod subagents_tests;

pub struct AntigravityProvider;

impl AntigravityProvider {
    pub const WIRED: &'static [Capability] = &[
        Capability::SubagentList,
        Capability::SubagentStatus,
        Capability::SubagentGlance,
        Capability::SubagentOpen,
    ];
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

    fn subagents(&self) -> &dyn Subagents {
        &subagents::AntigravitySubagents
    }
}
