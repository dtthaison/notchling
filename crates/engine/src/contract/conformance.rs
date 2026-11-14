use std::path::Path;

use super::{Capability, Registry, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    Passed,
    NotWritten,
}

pub fn check(_registry: &Registry, _tool: Tool, _c: Capability, _root: &Path) -> Checked {
    Checked::NotWritten
}
