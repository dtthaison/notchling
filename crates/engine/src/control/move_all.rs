//! Move All: putting every session on one account. An account is live in one place only, so the
//! sessions cannot follow it into a lane: background sessions in lanes stop and resume on the main
//! login, the lane holding the account closes (giving the login back), and the main login switches
//! to it. This is `MoveAllPlan` from `Chat/MoveAll.swift`; closing a lane and switching the login are
//! account work (milestone 6), so they stay data here.

use super::plan::{Outcome, Plan, Step};
use super::session::{conversation_of, flags_for_job, resume_plan_step, stop_for};
use crate::sessions::{config_dir_for_job, Agent, AgentTarget, Lane, Roots};
use std::collections::HashSet;

/// One background session in a lane, to stop and resume on the main login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveJob {
    pub agent_id: String,
    pub job_id: String,
    pub name: String,
    pub cwd: String,
    pub transcript: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveAllPlan {
    /// Background sessions in lanes, to stop and resume on the main login.
    pub jobs: Vec<MoveJob>,
    /// The lane that holds the account, closed so the main login can take it.
    pub close_lane: Option<String>,
    /// The account the main login switches to; `None` when it is on it already.
    pub switch_main: Option<String>,
    /// Terminal sessions in the lane that closes: they lose their login and cannot be moved.
    pub signed_out: Vec<String>,
}

impl MoveAllPlan {
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty() && self.close_lane.is_none() && self.switch_main.is_none()
    }

    /// `target` is the account to land on (`None`: the main login's own account); `active_id` is the
    /// main login's account.
    pub fn make(
        agents: &[Agent],
        lanes: &[Lane],
        active_id: Option<&str>,
        target: Option<&str>,
    ) -> MoveAllPlan {
        let target = if target == active_id { None } else { target };
        let lane_paths: HashSet<&str> = lanes.iter().map(|l| l.path.as_str()).collect();
        let closing = target.and_then(|id| lanes.iter().find(|l| l.account_id == id));
        let mut plan = MoveAllPlan {
            switch_main: target.map(String::from),
            close_lane: closing.map(|l| l.name.clone()),
            ..MoveAllPlan::default()
        };
        for agent in agents {
            let Some(dir) = agent.config_dir.as_deref() else {
                continue;
            };
            if !lane_paths.contains(dir) {
                continue;
            }
            match &agent.target {
                AgentTarget::Job(job_id) => {
                    // A stopped session stays where it is; it was stopped on purpose.
                    let Some(transcript) = agent.transcript_path.as_deref() else {
                        continue;
                    };
                    if agent.label == "Stopped" {
                        continue;
                    }
                    plan.jobs.push(MoveJob {
                        agent_id: agent.id.clone(),
                        job_id: job_id.clone(),
                        name: agent.name.clone(),
                        cwd: agent.cwd.clone(),
                        transcript: transcript.to_string(),
                    });
                }
                AgentTarget::Process(_) => {
                    if closing.is_some_and(|l| l.path == dir) {
                        plan.signed_out.push(agent.name.clone());
                    }
                }
                AgentTarget::Resume { .. } => {}
            }
        }
        plan
    }
}

/// The two halves of moving one session to the main login. The caller stops every job, does the
/// account work (close the lane, switch the login), then resumes every job that stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobMove {
    pub agent_id: String,
    pub stop: Plan,
    pub resume: Plan,
}

/// The stop and resume plans for each job of `plan`. The flags are read now, before the stop, from
/// the folder holding the job's current copy.
pub fn plan_move_all_jobs(plan: &MoveAllPlan, roots: &Roots) -> Vec<JobMove> {
    plan.jobs
        .iter()
        .map(|job| {
            let flags = flags_for_job(roots, &job.job_id, &job.name);
            let conversation = conversation_of(&job.transcript).unwrap_or_default();
            JobMove {
                agent_id: job.agent_id.clone(),
                stop: stop_for(
                    &job.job_id,
                    config_dir_for_job(roots, &job.job_id).as_deref(),
                ),
                resume: Plan {
                    steps: vec![Step::Run(resume_plan_step(
                        roots,
                        &job.job_id,
                        &conversation,
                        &job.cwd,
                        None,
                        &flags,
                    ))],
                    outcome: Outcome::NewJob,
                },
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "move_all_tests.rs"]
mod tests;
