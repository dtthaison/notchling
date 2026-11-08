//! The listing: `AgentMonitor.poll` without the process lookups.

use super::agent::Agent;
use super::lanes::Roots;
use std::collections::HashMap;

/// What the engine can't learn from files.
pub struct Probes<'a> {
    pub is_alive: &'a dyn Fn(u32) -> bool,
    pub host_app: &'a dyn Fn(u32) -> Option<String>,
}

/// Remembers what a poll saw, so an ended terminal session shows as Exited.
pub struct Monitor {
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
    pub fn poll(&mut self, _roots: &Roots, _probes: &Probes, _now_ms: i64) -> Vec<Agent> {
        let _ = (&self.last_live, &self.exited, &self.lane_of_session);
        todo!()
    }

    pub fn config_dir_for_session(&self, _id: &str) -> Option<String> {
        todo!()
    }
}
