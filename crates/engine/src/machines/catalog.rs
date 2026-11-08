//! The steps of a machine, in the order they show and run (Swift `StepCatalog`), and the real
//! [`Catalog`] the engine runs with.

use super::engine::Catalog;
use super::registry::{Machine, MachineOs};
use super::step::{SetupStep, StepId};
use super::{steps_machine, steps_repo, steps_sd, steps_secret, token_scope};
use std::sync::Arc;

/// Machine steps (Claude login after Claude Code), then, for the sd worker job, each repo's steps
/// and the job's own (the issues token and its scope after Plugins installed). An unknown OS keeps
/// the macOS steps; they wait for Reachable to tell.
pub fn steps(m: &Machine, all: &[Machine]) -> Vec<SetupStep> {
    let mut out = Vec::new();
    for step in steps_machine::all(m) {
        let after = step.id == StepId::CLAUDE_CODE;
        out.push(step);
        if after {
            out.push(steps_secret::claude_login(m));
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

/// The catalog of the real steps.
pub fn standard() -> Catalog {
    Catalog {
        steps: Arc::new(steps),
        usages: Arc::new(steps_secret::usages),
        deliver: Arc::new(steps_secret::deliver),
        toggle_script: Arc::new(steps_sd::toggle_script),
        offers_replace: Arc::new(steps_repo::offers_replace),
    }
}
