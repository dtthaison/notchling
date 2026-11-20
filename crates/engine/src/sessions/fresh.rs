use super::agent::Agent;
use super::iso::parse_iso_ms;
use super::jobs::{agent_from_job, pid_of, rank, read_json, read_sessions, Rank};
use super::lanes::{config_dirs, Roots};
use super::monitor::Probes;
use crate::transcript::util::modified_secs;
use serde_json::Value;
use std::path::Path;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    No,
    Unknown,
    Stopped,
    Done,
}

pub const MID_WRITE_MS: i64 = 10_000;
pub const START_SLACK_MS: i64 = 5_000;

pub struct Look<'a> {
    pub alive: &'a dyn Fn(u32) -> bool,
    pub started_ms: &'a dyn Fn(u32) -> Option<i64>,
    pub now_ms: i64,
}

impl Look<'_> {
    fn writing(&self, at: Option<i64>) -> bool {
        at.is_some_and(|at| (self.now_ms - at).abs() < MID_WRITE_MS)
    }

    fn runs(&self, session: &Value) -> bool {
        let Some(pid) = pid_of(session) else {
            return false;
        };
        if !(self.alive)(pid) {
            return false;
        }
        let written = session.get("startedAt").and_then(ms_of);
        match (written, (self.started_ms)(pid)) {
            (Some(written), Some(began)) => began <= written + START_SLACK_MS,
            _ => true,
        }
    }
}

pub fn job_ended(roots: &Roots, look: &Look, short_id: &str) -> Ended {
    if !named(short_id) {
        return Ended::No;
    }
    let configs = config_dirs(roots);
    let mut found = false;
    let mut writing = false;
    let mut best: Option<(Rank, String)> = None;
    for config in &configs {
        let dir = config.dir.join("jobs").join(short_id);
        if !dir.is_dir() {
            continue;
        }
        found = true;
        let path = dir.join("state.json");
        let Some(job) = read_json(&path) else {
            writing |= look.writing(modified_ms(&path).or_else(|| modified_ms(&dir)));
            continue;
        };
        let this = rank(&job);
        if best.as_ref().is_none_or(|(top, _)| this > *top) {
            let state = job.get("state").and_then(Value::as_str).unwrap_or("");
            best = Some((this, state.to_string()));
        }
    }
    if !found {
        return Ended::No;
    }
    let state = best.map(|(_, state)| state);
    let runs = configs.iter().any(|config| {
        read_sessions(&config.dir, &|_| true).iter().any(|session| {
            session.get("jobId").and_then(Value::as_str) == Some(short_id) && look.runs(session)
        })
    });
    if runs {
        return match state.as_deref() {
            Some("stopped") => Ended::Unknown,
            _ => Ended::No,
        };
    }
    if writing || configs.iter().any(|c| session_file_writing(&c.dir, look)) {
        return Ended::Unknown;
    }
    match state.as_deref() {
        None | Some("stopped") => Ended::Stopped,
        Some("done") => Ended::Done,
        Some(_) => Ended::No,
    }
}

fn modified_ms(path: &Path) -> Option<i64> {
    let at = std::fs::metadata(path).ok()?.modified().ok()?;
    let since = at.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

fn session_file_writing(base: &Path, look: &Look) -> bool {
    let Ok(entries) = std::fs::read_dir(base.join("sessions")) else {
        return false;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| look.writing(modified_ms(p)))
        .filter(|p| read_json(p).is_none())
        .any(|p| {
            p.file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u32>().ok())
                .is_none_or(look.alive)
        })
}

fn ms_of(at: &Value) -> Option<i64> {
    at.as_i64()
        .or_else(|| at.as_f64().map(|f| f as i64))
        .or_else(|| at.as_str().and_then(parse_iso_ms))
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
    Some((pid, live.get("startedAt").and_then(ms_of)))
}

#[cfg(test)]
#[path = "fresh_tests.rs"]
mod tests;
