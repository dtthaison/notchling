use super::engine_plan::{PlanKind, ShownPlan};
use super::step::StepId;
use super::steps_tailscale_linux::SIGN_IN;
use sha2::{Digest, Sha256};

pub const FAMILY_VAULT: &str = "vault";
pub const FAMILY_CLUSTER: &str = "cluster";
pub const FAMILY_TAILSCALE: &str = "tailscale";

const SECRET_OR_LOGIN: [StepId; 5] = [
    StepId::ISSUES_TOKEN,
    StepId::TOKEN_SCOPE,
    StepId::CLAUDE_LOGIN,
    StepId::NODE_CODEX_LOGIN,
    StepId::NODE_AGY_LOGIN,
];

pub fn step_refusal(id: &StepId) -> Option<&'static str> {
    if SECRET_OR_LOGIN.contains(id) {
        Some(FAMILY_VAULT)
    } else if *id == StepId::NODE_JOINED {
        Some(FAMILY_CLUSTER)
    } else {
        None
    }
}

pub fn kind_refusal(kind: &PlanKind) -> Option<&'static str> {
    match kind {
        PlanKind::Fix(id) | PlanKind::Replace(id) => step_refusal(id),
        PlanKind::SendAgain(_) => Some(FAMILY_VAULT),
        _ => None,
    }
}

pub fn fix_refusal(id: &StepId, display: &str) -> Option<&'static str> {
    if *id == StepId::NODE_TAILSCALE && display == SIGN_IN {
        Some(FAMILY_TAILSCALE)
    } else {
        step_refusal(id)
    }
}

pub fn refusal(plan: &ShownPlan) -> Option<&'static str> {
    kind_refusal(&plan.kind).or_else(|| {
        plan.steps
            .iter()
            .find_map(|s| fix_refusal(&s.id, &s.display))
    })
}

pub fn digest(plan: &ShownPlan) -> String {
    let mut hash = Sha256::new();
    hash.update(format!("{:?}|{}|{:?}", plan.kind, plan.machine, plan.admin).as_bytes());
    for step in &plan.steps {
        hash.update(b"\0");
        hash.update(step.id.raw().as_bytes());
        hash.update(b"\0");
        hash.update(step.title.as_bytes());
        hash.update(b"\0");
        hash.update(step.display.as_bytes());
        hash.update(if step.needs_admin { b"1" } else { b"0" });
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
