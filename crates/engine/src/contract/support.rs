use super::Capability;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flow {
    HeldQueue,
    HideRow,
    DelegateJobs,
    ProcessTree,
    CatalogData,
    VersionProbe,
    VersionCache,
    TitleIndex,
}

impl Flow {
    pub const ALL: [Flow; 8] = [
        Flow::HeldQueue,
        Flow::HideRow,
        Flow::DelegateJobs,
        Flow::ProcessTree,
        Flow::CatalogData,
        Flow::VersionProbe,
        Flow::VersionCache,
        Flow::TitleIndex,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Flow::HeldQueue => "held-queue",
            Flow::HideRow => "hide-row",
            Flow::DelegateJobs => "delegate-jobs",
            Flow::ProcessTree => "process-tree",
            Flow::CatalogData => "catalog-data",
            Flow::VersionProbe => "version-probe",
            Flow::VersionCache => "version-cache",
            Flow::TitleIndex => "title-index",
        }
    }

    pub fn covers(self, c: Capability) -> bool {
        match self {
            Flow::HeldQueue => matches!(c, Capability::TurnHold | Capability::TurnTakeBack),
            Flow::HideRow => c == Capability::SessionRemove,
            Flow::DelegateJobs => matches!(
                c,
                Capability::SubagentList
                    | Capability::SubagentStatus
                    | Capability::SubagentGlance
                    | Capability::SubagentOpen
                    | Capability::SubagentStop
                    | Capability::SignalSubagentFinished
            ),
            Flow::ProcessTree => c == Capability::DelegateCaller,
            Flow::CatalogData => matches!(
                c,
                Capability::CatalogModels | Capability::ConversationContext
            ),
            Flow::VersionProbe => c == Capability::VersionDetect,
            Flow::VersionCache => c == Capability::VersionUpdate,
            Flow::TitleIndex => c == Capability::SearchText,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Native,
    Fallback,
    NotApplicable,
    Gap,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Native => "native",
            Kind::Fallback => "fallback",
            Kind::NotApplicable => "not_applicable",
            Kind::Gap => "gap",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Native {
        how: &'static str,
    },
    Fallback {
        flow: Flow,
        why: &'static str,
    },
    NotApplicable {
        why: &'static str,
    },
    Gap {
        plan: &'static str,
        meanwhile: Option<Flow>,
    },
}

impl Support {
    pub const fn native(how: &'static str) -> Support {
        Support::Native { how }
    }

    pub const fn fallback(flow: Flow, why: &'static str) -> Support {
        Support::Fallback { flow, why }
    }

    pub const fn not_applicable(why: &'static str) -> Support {
        Support::NotApplicable { why }
    }

    pub const fn gap(plan: &'static str) -> Support {
        Support::Gap {
            plan,
            meanwhile: None,
        }
    }

    pub const fn gap_meanwhile(plan: &'static str, flow: Flow) -> Support {
        Support::Gap {
            plan,
            meanwhile: Some(flow),
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Support::Native { .. } => Kind::Native,
            Support::Fallback { .. } => Kind::Fallback,
            Support::NotApplicable { .. } => Kind::NotApplicable,
            Support::Gap { .. } => Kind::Gap,
        }
    }

    pub fn line(&self) -> &'static str {
        match self {
            Support::Native { how } => how,
            Support::Fallback { why, .. } => why,
            Support::NotApplicable { why } => why,
            Support::Gap { plan, .. } => plan,
        }
    }

    pub fn flow(&self) -> Option<Flow> {
        match self {
            Support::Fallback { flow, .. } => Some(*flow),
            Support::Gap { meanwhile, .. } => *meanwhile,
            Support::Native { .. } | Support::NotApplicable { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub capability: Capability,
    pub why: &'static str,
}

impl Unsupported {
    pub fn not_native(capability: Capability) -> Unsupported {
        Unsupported {
            capability,
            why: "not native",
        }
    }
}
