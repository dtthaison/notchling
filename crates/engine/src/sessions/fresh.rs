use super::agent::Agent;
use super::iso::parse_iso_ms;
use super::jobs::{agent_from_job, pid_of, rank, read_json, read_sessions, Rank};
use super::lanes::{config_dirs, Roots};
use super::monitor::Probes;
use crate::transcript::util::modified_secs;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
pub const PROC_START_SLACK_MS: i64 = 2_000;
const FORGET_MS: i64 = 600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Process {
    Gone,
    Other,
    Ours {
        started_ms: Option<i64>,
        ticks: Option<u64>,
    },
}

type Stamp = Option<(u64, Option<SystemTime>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub path: PathBuf,
    pub stamp: Stamp,
}

impl Mark {
    pub fn of(path: &Path) -> Mark {
        Mark {
            path: path.to_path_buf(),
            stamp: std::fs::metadata(path)
                .ok()
                .map(|meta| (meta.len(), meta.modified().ok())),
        }
    }
}

struct Sight {
    stamp: Stamp,
    since_ms: i64,
    last_ms: i64,
}

#[derive(Default)]
pub struct Watch {
    seen: HashMap<PathBuf, Sight>,
}

impl Watch {
    pub fn settled(&mut self, mark: &Mark, now_ms: i64) -> bool {
        self.seen
            .retain(|_, sight| now_ms - sight.last_ms < FORGET_MS);
        match self.seen.get_mut(&mark.path) {
            Some(sight) if sight.stamp == mark.stamp => {
                sight.last_ms = now_ms;
                now_ms - sight.since_ms >= MID_WRITE_MS
            }
            _ => {
                let sight = Sight {
                    stamp: mark.stamp,
                    since_ms: now_ms,
                    last_ms: now_ms,
                };
                self.seen.insert(mark.path.clone(), sight);
                false
            }
        }
    }
}

pub struct Look<'a> {
    pub process: &'a dyn Fn(u32) -> Process,
    pub settled: &'a dyn Fn(&Mark) -> bool,
}

impl Look<'_> {
    fn runs(&self, session: &Value) -> bool {
        let Some(pid) = pid_of(session) else {
            return false;
        };
        let Process::Ours { started_ms, ticks } = (self.process)(pid) else {
            return false;
        };
        let token = session
            .get("procStart")
            .and_then(Value::as_str)
            .map(str::trim);
        let counted = token.and_then(|token| token.parse::<u64>().ok());
        let dated = token.and_then(lstart_ms);
        let written = session.get("startedAt").and_then(ms_of);
        let same: Vec<bool> = [
            ticks.zip(counted).map(|(now, then)| now == then),
            started_ms
                .zip(dated)
                .map(|(now, then)| (now - then).abs() <= PROC_START_SLACK_MS),
            started_ms
                .zip(written)
                .map(|(began, written)| began <= written + START_SLACK_MS),
        ]
        .into_iter()
        .flatten()
        .collect();
        same.is_empty() || same.contains(&true)
    }

    fn holds(&self, path: &Path) -> bool {
        let mark = Mark::of(path);
        if mark.stamp.is_none() {
            return false;
        }
        let pid = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok());
        if pid.is_some_and(|pid| !matches!((self.process)(pid), Process::Ours { .. })) {
            return false;
        }
        !(self.settled)(&mark)
    }
}

pub fn job_ended(roots: &Roots, look: &Look, short_id: &str) -> Ended {
    if !named(short_id) {
        return Ended::No;
    }
    let configs = config_dirs(roots);
    let mut found = false;
    let mut held = false;
    let mut best: Option<(Rank, String)> = None;
    for config in &configs {
        let dir = config.dir.join("jobs").join(short_id);
        if !dir.is_dir() {
            continue;
        }
        found = true;
        let path = dir.join("state.json");
        let Some(job) = read_json(&path) else {
            held |= !(look.settled)(&Mark::of(&path));
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
    let mut live = false;
    for config in &configs {
        let Some(files) = session_files(&config.dir) else {
            held = true;
            continue;
        };
        for path in files {
            match read_json(&path) {
                Some(session) => {
                    live |= session.get("jobId").and_then(Value::as_str) == Some(short_id)
                        && look.runs(&session);
                }
                None => held |= look.holds(&path),
            }
        }
    }
    match (state.as_deref(), live, held) {
        (Some("stopped"), true, _) => Ended::Unknown,
        (_, true, _) => Ended::No,
        (None, false, _) => Ended::Unknown,
        (Some("stopped" | "done"), false, true) => Ended::Unknown,
        (Some("stopped"), false, false) => Ended::Stopped,
        (Some("done"), false, false) => Ended::Done,
        (Some(_), false, _) => Ended::No,
    }
}

fn session_files(base: &Path) -> Option<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(base.join("sessions")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    Some(files)
}

fn lstart_ms(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = text.split_whitespace().skip(1);
    let name = parts.next()?;
    let month = MONTHS.iter().position(|m| *m == name)? + 1;
    let day: u32 = parts.next()?.parse().ok()?;
    let clock = parts.next()?;
    let year: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    parse_iso_ms(&format!("{year:04}-{month:02}-{day:02}T{clock}Z"))
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

#[cfg(test)]
#[path = "fresh_ended_tests.rs"]
mod ended_tests;
