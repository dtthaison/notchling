//! The listing: `AgentMonitor.poll` without the process lookups.

use super::agent::{Agent, AgentGroup, AgentTarget};
use super::jobs::{agent_from_job, agent_from_session, rank, read_jobs, read_sessions, Rank};
use super::lanes::{config_dirs, Roots};
use super::successor::file_modified_ms;
use serde_json::Value;
use std::collections::HashMap;

/// What the engine can't learn from files.
pub struct Probes<'a> {
    /// Whether a process id is alive (a `sessions/<pid>.json` of a dead process is skipped).
    pub is_alive: &'a dyn Fn(u32) -> bool,
    /// The app hosting a terminal session's process ("Terminal", "iTerm"), `None` when unknown.
    pub host_app: &'a dyn Fn(u32) -> Option<String>,
}

impl Probes<'static> {
    /// The default without platform calls: liveness from Linux's `/proc` (everywhere else every pid is
    /// assumed alive, so a stale file shows until the app adds a real probe) and no host app.
    pub fn from_proc() -> Self {
        Probes { is_alive: &proc_alive, host_app: &no_host }
    }
}

fn proc_alive(pid: u32) -> bool {
    let proc = std::path::Path::new("/proc");
    !proc.join("self").exists() || proc.join(pid.to_string()).exists()
}

fn no_host(_: u32) -> Option<String> {
    None
}

/// Remembers what earlier polls saw: a terminal session that ended shows as "Exited" with a resume
/// target, and each session's lane is kept so an ended one resumes where it ran.
pub struct Monitor {
    /// Completed rows older than this (milliseconds) are hidden. Default 24 hours.
    pub completed_window_ms: i64,
    last_live: HashMap<String, Agent>,
    exited: HashMap<String, Agent>,
    lane_of_session: HashMap<String, String>,
}

impl Default for Monitor {
    fn default() -> Self {
        Monitor {
            completed_window_ms: 24 * 3600 * 1000,
            last_live: HashMap::new(),
            exited: HashMap::new(),
            lane_of_session: HashMap::new(),
        }
    }
}

impl Monitor {
    /// Reads every config folder (the main one, then each lane's) and returns the rows: needs input,
    /// then working, then completed, newest first within each. `now_ms` is the clock.
    pub fn poll(&mut self, roots: &Roots, probes: &Probes, now_ms: i64) -> Vec<Agent> {
        let mut result: Vec<Agent> = Vec::new();
        let mut live: HashMap<String, Agent> = HashMap::new();
        let mut lanes: HashMap<String, String> = HashMap::new();
        let mut main_ids: Vec<String> = Vec::new();
        // One row per conversation: a moved session's stopped copy in the folder it left is hidden.
        let mut job_rank: HashMap<String, Rank> = HashMap::new();

        for config in config_dirs(roots) {
            let lane_path = config.lane.as_ref().map(|l| l.path.clone());
            let sessions = read_sessions(&config.dir, probes.is_alive);
            let jobs = read_jobs(&config.dir);

            let mut session_by_job: HashMap<String, &Value> = HashMap::new();
            for s in &sessions {
                if let Some(job_id) = s.get("jobId").and_then(Value::as_str) {
                    session_by_job.insert(job_id.to_string(), s);
                }
            }

            for (short_id, job) in &jobs {
                let live_session = session_by_job.get(short_id).copied();
                let mut agent = agent_from_job(job, short_id, live_session, &file_modified_ms);
                agent.config_dir = lane_path.clone();
                let this = rank(job);
                if let Some(index) = result.iter().position(|a| a.id == agent.id) {
                    match job_rank.get(&agent.id) {
                        Some(best) if this > *best => result[index] = agent.clone(),
                        _ => continue,
                    }
                } else {
                    result.push(agent.clone());
                }
                job_rank.insert(agent.id.clone(), this);
                match &lane_path {
                    Some(path) => {
                        lanes.insert(agent.id.clone(), path.clone());
                    }
                    None => {
                        lanes.remove(&agent.id);
                        main_ids.push(agent.id.clone());
                    }
                }
            }

            for s in &sessions {
                let job_id = s.get("jobId").and_then(Value::as_str);
                if job_id.is_some_and(|id| jobs.contains_key(id)) {
                    continue;
                }
                // A terminal session that handed its work to a background job is shown as that job.
                let parked = s.get("parkedJobId").and_then(Value::as_str);
                if parked.is_some_and(|id| jobs.contains_key(id)) {
                    continue;
                }
                let Some(mut agent) = agent_from_session(s, probes.host_app) else { continue };
                agent.config_dir = lane_path.clone();
                self.exited.remove(&agent.id);
                if let Some(path) = &lane_path {
                    lanes.insert(agent.id.clone(), path.clone());
                }
                live.insert(agent.id.clone(), agent);
            }
        }

        self.lane_of_session.extend(lanes.clone());
        // Moved back to the main login: forget the lane it left.
        for id in main_ids {
            if !lanes.contains_key(&id) {
                self.lane_of_session.remove(&id);
            }
        }

        let job_ids: std::collections::HashSet<&str> = result.iter().map(|a| a.id.as_str()).collect();
        for (id, old) in &self.last_live {
            if live.contains_key(id) || job_ids.contains(id.as_str()) {
                continue;
            }
            self.exited.insert(
                id.clone(),
                Agent {
                    group: AgentGroup::Completed,
                    label: "Exited".to_string(),
                    since_ms: now_ms,
                    target: AgentTarget::Resume { session_id: id.clone(), cwd: old.cwd.clone() },
                    suggested_reply: None,
                    transcript_path: None,
                    host: String::new(),
                    headless: false,
                    command: None,
                    resume_session_id: None,
                    ..old.clone()
                },
            );
        }
        self.last_live = live.clone();

        result.extend(live.into_values());
        result.extend(self.exited.values().cloned());

        let cutoff = now_ms - self.completed_window_ms;
        result.retain(|a| a.group != AgentGroup::Completed || a.since_ms > cutoff);
        result.sort_by(|a, b| a.group.cmp(&b.group).then(b.since_ms.cmp(&a.since_ms)).then(a.id.cmp(&b.id)));
        result
    }

    /// The lane folder a session was last seen in, `None` for the main login or an unseen session.
    pub fn config_dir_for_session(&self, id: &str) -> Option<String> {
        self.lane_of_session.get(id).cloned()
    }
}
