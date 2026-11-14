use super::agent::Agent;
use super::jobs::{agent_from_job, rank, read_json, read_sessions};
use super::lanes::{config_dirs, Roots};
use super::monitor::Probes;
use crate::transcript::util::modified_secs;
use serde_json::Value;

pub fn job_row(roots: &Roots, probes: &Probes, short_id: &str) -> Option<Agent> {
    if short_id.is_empty() || short_id.contains(['/', '\\']) || short_id.starts_with('.') {
        return None;
    }
    let mut best = None;
    for config in config_dirs(roots) {
        let path = config.dir.join("jobs").join(short_id).join("state.json");
        let Some(job) = read_json(&path) else {
            continue;
        };
        let sessions = read_sessions(&config.dir, probes.is_alive);
        let live = sessions
            .iter()
            .find(|s| s.get("jobId").and_then(Value::as_str) == Some(short_id));
        let mut agent = agent_from_job(&job, short_id, live, &modified_secs);
        agent.config_dir = config.lane.as_ref().map(|l| l.path.clone());
        let this = rank(&job);
        if best.as_ref().is_none_or(|(top, _)| this > *top) {
            best = Some((this, agent));
        }
    }
    best.map(|(_, agent)| agent)
}

#[cfg(test)]
#[path = "fresh_tests.rs"]
mod tests;
