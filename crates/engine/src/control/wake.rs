use crate::sessions::{Agent, AgentGroup, AgentTarget};
use crate::settings::Settings;
use std::collections::HashSet;

pub const PROMPT: &str =
    "The Mac went to sleep and cut off your last reply. Continue where you stopped.";
pub const SETTLE_MS: i64 = 20_000;
pub const WINDOW_MS: i64 = 30 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nudge {
    pub agent_id: String,
    pub job_id: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct WakeResumer {
    woke_at_ms: Option<i64>,
    handled: HashSet<String>,
    sending: HashSet<String>,
}

impl WakeResumer {
    pub fn new() -> Self {
        WakeResumer::default()
    }

    pub fn did_wake(&mut self, now_ms: i64) {
        self.woke_at_ms = Some(now_ms);
    }

    pub fn waiting(&self) -> bool {
        self.woke_at_ms.is_some()
    }

    pub fn finished(&mut self, agent_id: &str) {
        self.sending.remove(agent_id);
    }

    pub fn check(
        &mut self,
        agents: &[Agent],
        now_ms: i64,
        enabled: bool,
        cut_off: &dyn Fn(&Agent) -> Option<String>,
    ) -> Vec<Nudge> {
        let Some(woke) = self.woke_at_ms.filter(|_| enabled) else {
            return Vec::new();
        };
        let since = now_ms - woke;
        if since < SETTLE_MS {
            return Vec::new();
        }
        if since > WINDOW_MS {
            self.woke_at_ms = None;
            return Vec::new();
        }
        let mut nudges = Vec::new();
        for agent in agents.iter().filter(|a| a.group != AgentGroup::Working) {
            let AgentTarget::Job(job_id) = &agent.target else {
                continue;
            };
            if self.sending.contains(&agent.id) {
                continue;
            }
            let Some(cut) = cut_off(agent) else {
                continue;
            };
            if !self.handled.insert(cut) {
                continue;
            }
            self.sending.insert(agent.id.clone());
            nudges.push(Nudge {
                agent_id: agent.id.clone(),
                job_id: job_id.clone(),
                text: PROMPT.to_string(),
            });
        }
        nudges
    }
}

pub const KEY: &str = "continueAfterSleep";

pub fn enabled(settings: &Settings) -> bool {
    settings.get_bool(KEY).unwrap_or(true)
}

pub fn sleep_cut(agent: &Agent) -> Option<String> {
    crate::transcript::errors::sleep_cut_in_file(agent.transcript_path.as_deref()?)
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "wake_setting_tests.rs"]
mod setting_tests;
