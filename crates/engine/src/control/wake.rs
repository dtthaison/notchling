//! Resume after wake (`WakeResumer`): after the Mac wakes, tell background agents whose reply was
//! cut off by sleep to carry on.
//!
//! Closing the lid mid-reply ends the turn with "API Error: Your computer went to sleep
//! mid-response" and the session then sits idle until someone writes to it. Only background jobs can
//! be written to (`claude attach`); terminal sessions belong to their window and `claude -p` runs
//! have ended. This is the pure decision: the clock and the cut-off check come in as arguments, and
//! the answer is who to nudge. Sending the text is the reply planner's job.

use crate::sessions::{Agent, AgentGroup, AgentTarget};
use std::collections::HashSet;

/// What the agent is told.
pub const PROMPT: &str =
    "The Mac went to sleep and cut off your last reply. Continue where you stopped.";
/// Give the network and the background service time to come back before writing.
pub const SETTLE_MS: i64 = 20_000;
/// Look for cut-off replies this long after a wake; later ones were not caused by this sleep.
pub const WINDOW_MS: i64 = 30 * 60 * 1000;

/// One agent to nudge: type `text` into job `job_id` as a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nudge {
    pub agent_id: String,
    pub job_id: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct WakeResumer {
    woke_at_ms: Option<i64>,
    /// Error entries already answered, so each cut-off gets one nudge.
    handled: HashSet<String>,
    sending: HashSet<String>,
}

impl WakeResumer {
    pub fn new() -> Self {
        WakeResumer::default()
    }

    /// The Mac woke at this time (milliseconds since the epoch).
    pub fn did_wake(&mut self, now_ms: i64) {
        self.woke_at_ms = Some(now_ms);
    }

    /// The nudge for `agent_id` went out (or failed): it may be nudged again for a new cut-off.
    pub fn finished(&mut self, agent_id: &str) {
        self.sending.remove(agent_id);
    }

    /// Called from the polling loop with the current agents. `enabled` is the user's setting
    /// (`continueAfterSleep`, on by default); `cut_off` gives the id of the transcript entry that
    /// ended the agent's last reply when it is the sleep error, nothing else.
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

/// The default cut-off check: the sleep error ending the agent's transcript.
pub fn sleep_cut(agent: &Agent) -> Option<String> {
    crate::transcript::errors::sleep_cut_in_file(agent.transcript_path.as_deref()?)
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;
