use std::sync::Arc;

use super::subagents::Subagents;
use super::{Capability, Provider, Support, Tool};
use crate::tools::codex::children::ChildSource;

pub mod subagents;
pub mod support;

#[cfg(test)]
mod subagents_tests;

pub struct CodexProvider {
    subagents: subagents::CodexSubagents,
}

impl CodexProvider {
    pub fn new(source: Arc<dyn ChildSource>) -> CodexProvider {
        CodexProvider {
            subagents: subagents::CodexSubagents::new(source),
        }
    }

    pub const WIRED: &'static [Capability] = &[
        Capability::SubagentList,
        Capability::SubagentStatus,
        Capability::SubagentGlance,
        Capability::SubagentOpen,
    ];
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

    fn subagents(&self) -> &dyn Subagents {
        &self.subagents
    }
}
