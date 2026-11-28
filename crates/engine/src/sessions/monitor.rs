use super::agent::{Agent, AgentGroup};
use super::exited_store::{ExitedStore, Row};
use super::jobs::{agent_from_job, agent_from_session, rank, read_jobs, read_sessions, Rank};
use super::lanes::{config_dirs, Roots};
use crate::settings::KeepFinished;
use crate::transcript::util::modified_secs;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub struct Probes<'a> {
    pub is_alive: &'a dyn Fn(u32) -> bool,
    pub host_app: &'a dyn Fn(u32) -> Option<String>,
}

impl Probes<'static> {
    pub fn from_proc() -> Self {
        Probes {
            is_alive: &pid_alive,
            host_app: &no_host,
        }
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid == 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; `kill` only checks that the process exists.
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(not(unix))]
fn pid_alive(_: u32) -> bool {
    true
}

fn no_host(_: u32) -> Option<String> {
    None
}

pub struct Monitor {
    pub completed_window_ms: i64,
    last_live: HashMap<String, Agent>,
    exited: HashMap<String, Agent>,
    lane_of_session: HashMap<String, String>,
    store: Option<ExitedStore>,
}

impl Default for Monitor {
    fn default() -> Self {
        Monitor {
            completed_window_ms: KeepFinished::default().window_ms(),
            last_live: HashMap::new(),
            exited: HashMap::new(),
            lane_of_session: HashMap::new(),
            store: None,
        }
    }
}

impl Monitor {
    pub fn with_exited_store(mut self, store: ExitedStore) -> Monitor {
        for agent in store.load() {
            self.exited.insert(agent.id.clone(), agent);
        }
        self.store = Some(store);
        self
    }

    fn exited_ids(&self) -> HashSet<String> {
        self.exited.keys().cloned().collect()
    }

    pub fn poll(&mut self, roots: &Roots, probes: &Probes, now_ms: i64) -> Vec<Agent> {
        let mut result: Vec<Agent> = Vec::new();
        let mut live: HashMap<String, Agent> = HashMap::new();
        let mut lanes: HashMap<String, String> = HashMap::new();
        let mut main_ids: Vec<String> = Vec::new();
        let mut job_rank: HashMap<String, Rank> = HashMap::new();
        let exited_before = self.store.as_ref().map(|_| self.exited_ids());
        let mut claimed: HashSet<String> = HashSet::new();
        let mut job_ended: HashSet<String> = HashSet::new();

        for config in config_dirs(roots) {
            let lane_path = config.lane.as_ref().map(|l| l.path.clone());
            let sessions = read_sessions(&config.dir, probes.is_alive);
            let jobs = read_jobs(&config.dir);

            let mut session_by_job: HashMap<String, &Value> = HashMap::new();
            let mut parked_jobs: HashSet<&str> = HashSet::new();
            for s in &sessions {
                if let Some(job_id) = s.get("jobId").and_then(Value::as_str) {
                    session_by_job.insert(job_id.to_string(), s);
                }
                if let Some(job_id) = s.get("parkedJobId").and_then(Value::as_str) {
                    parked_jobs.insert(job_id);
                }
            }

            for (short_id, job) in &jobs {
                let live_session = session_by_job.get(short_id).copied();
                let mut agent = agent_from_job(job, short_id, live_session, &modified_secs);
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
                let ended = matches!(
                    job.get("state").and_then(Value::as_str),
                    Some("stopped" | "done")
                ) && !session_by_job.contains_key(short_id)
                    && !parked_jobs.contains(short_id.as_str());
                if ended {
                    job_ended.insert(agent.id.clone());
                } else {
                    job_ended.remove(&agent.id);
                }
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
                let parked = s.get("parkedJobId").and_then(Value::as_str);
                if job_id.is_some_and(|id| jobs.contains_key(id))
                    || parked.is_some_and(|id| jobs.contains_key(id))
                {
                    if let Some(id) = s.get("sessionId").and_then(Value::as_str) {
                        claimed.insert(id.to_string());
                    }
                    continue;
                }
                let Some(mut agent) = agent_from_session(s, probes.host_app) else {
                    continue;
                };
                agent.config_dir = lane_path.clone();
                self.exited.remove(&agent.id);
                if let Some(path) = &lane_path {
                    lanes.insert(agent.id.clone(), path.clone());
                }
                live.insert(agent.id.clone(), agent);
            }
        }

        self.lane_of_session.extend(lanes.clone());
        for id in main_ids {
            if !lanes.contains_key(&id) {
                self.lane_of_session.remove(&id);
            }
        }

        let cutoff = now_ms - self.completed_window_ms;
        for agent in &result {
            if agent.group == AgentGroup::Completed && agent.since_ms <= cutoff {
                continue;
            }
            claimed.insert(agent.id.clone());
            if !job_ended.contains(&agent.id) {
                if let Some(resumed) = &agent.resume_session_id {
                    claimed.insert(resumed.clone());
                }
            }
        }
        self.exited.retain(|id, _| !claimed.contains(id));
        for (id, old) in &self.last_live {
            if live.contains_key(id) || claimed.contains(id) {
                continue;
            }
            self.exited.insert(id.clone(), Row::of(old, now_ms).agent());
        }
        self.exited.retain(|_, agent| agent.since_ms > cutoff);
        if let (Some(store), Some(before)) = (&self.store, exited_before) {
            if before != self.exited_ids() {
                let kept: Vec<Agent> = self.exited.values().cloned().collect();
                store.save(&kept);
            }
        }
        self.last_live = live.clone();

        result.extend(live.into_values());
        result.extend(self.exited.values().cloned());

        result.retain(|a| a.group != AgentGroup::Completed || a.since_ms > cutoff);
        result.sort_by(|a, b| {
            a.group
                .cmp(&b.group)
                .then(b.since_ms.cmp(&a.since_ms))
                .then(a.id.cmp(&b.id))
        });
        result
    }

    pub fn config_dir_for_session(&self, id: &str) -> Option<String> {
        self.lane_of_session.get(id).cloned()
    }
}
