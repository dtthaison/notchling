pub mod agent;
pub mod command_job;
pub mod iso;
pub mod jobs;
pub mod lanes;
pub mod monitor;

#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod monitor_tests;
#[cfg(test)]
pub(crate) mod testutil;

pub use agent::{Agent, AgentGroup, AgentTarget};
pub use jobs::{config_dir_for_job, job_dirs, launch_flags, resume_arguments};
pub use lanes::{config_dirs, read_lanes, ConfigDir, Lane, Roots};
pub use monitor::{Monitor, Probes};

pub fn list(roots: &Roots, probes: &Probes, now_ms: i64) -> Vec<Agent> {
    Monitor::default().poll(roots, probes, now_ms)
}
