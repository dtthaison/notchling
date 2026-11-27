use std::path::Path;

use super::conformance_subagents;
use super::{Capability, Registry, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    Passed,
    NotWritten,
}

pub fn check(registry: &Registry, tool: Tool, c: Capability, root: &Path) -> Checked {
    match c {
        Capability::SubagentList
        | Capability::SubagentStatus
        | Capability::SubagentGlance
        | Capability::SubagentOpen => conformance_subagents::check(registry, tool, c, root),
        _ => Checked::NotWritten,
    }
}
