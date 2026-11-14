use notchling_engine::contract::{Capability, Kind, Registry, Tool};
use notchling_locale as t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Fallback,
    NotApplicable,
    Gap,
}

impl RowState {
    pub fn word(self) -> &'static str {
        match self {
            RowState::Fallback => t::settings::capability_fallback(),
            RowState::NotApplicable => t::settings::capability_not_applicable(),
            RowState::Gap => t::settings::capability_gap(),
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
