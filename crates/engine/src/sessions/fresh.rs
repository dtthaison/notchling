use super::agent::Agent;
use super::iso::parse_iso_ms;
use super::jobs::{agent_from_job, rank, read_json, read_sessions};
use super::lanes::{config_dirs, Roots};
use super::monitor::Probes;
use crate::transcript::util::modified_secs;
use serde_json::Value;

fn named(short_id: &str) -> bool {
    !(short_id.is_empty() || short_id.contains(['/', '\\']) || short_id.starts_with('.'))
}

pub fn job_row(roots: &Roots, probes: &Probes, short_id: &str) -> Option<Agent> {
    if !named(short_id) {
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

pub fn job_state(roots: &Roots, short_id: &str) -> Option<String> {
    if !named(short_id) {
        return None;
    }
    let mut found = false;
    let mut best = None;
    for config in config_dirs(roots) {
        let dir = config.dir.join("jobs").join(short_id);
        if !dir.is_dir() {
            continue;
        }
        found = true;
        let Some(job) = read_json(&dir.join("state.json")) else {
            continue;
        };
        let this = rank(&job);
        if best.as_ref().is_none_or(|(top, _)| this > *top) {
            let state = job.get("state").and_then(Value::as_str).unwrap_or("");
            best = Some((this, state.to_string()));
        }
    }
    best.map(|(_, state)| state)
        .or_else(|| found.then(String::new))
}

fn live_file(roots: &Roots, alive: &dyn Fn(u32) -> bool, short_id: &str) -> Option<Value> {
    if !named(short_id) {
        return None;
    }
    config_dirs(roots).into_iter().find_map(|config| {
        read_sessions(&config.dir, alive)
            .into_iter()
            .find(|s| s.get("jobId").and_then(Value::as_str) == Some(short_id))
    })
}

pub fn job_status(roots: &Roots, probes: &Probes, short_id: &str) -> Option<String> {
    let live = live_file(roots, probes.is_alive, short_id)?;
    live.get("status").and_then(Value::as_str).map(String::from)
}

pub fn job_process(roots: &Roots, probes: &Probes, short_id: &str) -> Option<(u32, Option<i64>)> {
    let live = live_file(roots, probes.is_alive, short_id)?;
    let pid = live.get("pid").and_then(|n| {
        n.as_u64()
            .or_else(|| n.as_f64().map(|f| f as u64))
            .and_then(|p| u32::try_from(p).ok())
    })?;
    let started = live.get("startedAt").and_then(|at| {
        at.as_i64()
            .or_else(|| at.as_f64().map(|f| f as i64))
            .or_else(|| at.as_str().and_then(parse_iso_ms))
    });
    Some((pid, started))
}

#[cfg(test)]
#[path = "fresh_tests.rs"]
mod tests;
