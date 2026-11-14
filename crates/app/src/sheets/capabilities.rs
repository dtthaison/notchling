use notchling_engine::contract::{Capability, Kind, Registry, Tool};

pub const TITLE: &str = "What Notchling Can Do";

pub const FOOTER: &str =
    "Anything not listed works natively for that tool. notchling providers prints every row.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Fallback,
    NotApplicable,
    Gap,
}

impl RowState {
    pub fn word(self) -> &'static str {
        match self {
            RowState::Fallback => "Notchling's own",
            RowState::NotApplicable => "Not applicable",
            RowState::Gap => "Not built yet",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRow {
    pub capability: &'static str,
    pub state: RowState,
    pub line: &'static str,
}

pub fn rows(tool: Tool) -> Vec<CapabilityRow> {
    let registry = Registry::standard();
    Capability::ALL
        .into_iter()
        .filter_map(|c| {
            let support = registry.support(tool, c);
            let state = match support.kind() {
                Kind::Native => return None,
                Kind::Fallback => RowState::Fallback,
                Kind::NotApplicable => RowState::NotApplicable,
                Kind::Gap => RowState::Gap,
            };
            Some(CapabilityRow {
                capability: c.title(),
                state,
                line: support.line(),
            })
        })
        .collect()
}

pub fn parts() -> Vec<(Tool, Vec<CapabilityRow>)> {
    Tool::ALL.into_iter().map(|t| (t, rows(t))).collect()
}
