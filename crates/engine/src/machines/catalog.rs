use super::engine::Catalog;
use super::registry::{Machine, MachineOs};
use super::step::{SetupStep, StepId};
use super::{
    steps_machine, steps_repo, steps_sd, steps_secret, steps_selfupdate, steps_versions,
    token_scope,
};
use std::sync::Arc;

pub fn steps(m: &Machine, all: &[Machine]) -> Vec<SetupStep> {
    let mut out = Vec::new();
    for step in steps_machine::all(m) {
        let after = step.id == StepId::CLAUDE_CODE;
        out.push(step);
        if after {
            out.push(steps_secret::claude_login(m));
            out.push(steps_versions::tool_versions(m));
            out.extend(steps_selfupdate::steps(m));
        }
    }
    if let Some(job) = m.sd_worker() {
        for repo in &job.repos {
            out.extend(steps_repo::all(m, repo));
        }
        for step in steps_sd::all(m, job, all) {
            let after = step.id == StepId::PLUGINS;
            out.push(step);
            if after {
                out.push(steps_secret::issues_token(m, job));
                out.push(token_scope::step(m, job));
            }
        }
    }
    if m.os == Some(MachineOs::Linux) {
        let mac_only = [StepId::STAYS_AWAKE, StepId::FILEVAULT, StepId::XCODE];
        out.retain(|s| !mac_only.contains(&s.id));
    }
    out
}

pub fn standard() -> Catalog {
    Catalog {
        steps: Arc::new(steps),
        usages: Arc::new(steps_secret::usages),
        deliver: Arc::new(steps_secret::deliver),
        toggle_script: Arc::new(steps_sd::toggle_script),
        offers_replace: Arc::new(steps_repo::offers_replace),
    }
}
