//! E2: background jobs across lanes, `state.json`, `/clear` successors. Read-only, pure over a root
//! path, with no process spawning (that is E11/E16).
//!
//! # Public API
//!
//! - [`Roots::new(home, data_dir)`](Roots::new): where to read. `<home>/.claude` plus the lanes in
//!   `<data_dir>/lanes.json`.
//! - [`list`]: one poll with a fresh [`Monitor`] (no "Exited" memory). A read-only Chat calls this.
//! - [`Monitor::poll`]: the same, keeping state between polls so a terminal session that ended shows
//!   as "Exited" with a resume target, and remembering which lane each session ran in
//!   ([`Monitor::config_dir_for_session`]).
//! - [`Probes`]: what files can't tell: whether a pid is alive and which app hosts a terminal.
//!   [`Probes::from_proc`] is the default (Linux `/proc`; elsewhere it assumes alive).
//! - [`Agent`], [`AgentGroup`], [`AgentTarget`]: the row.
//! - [`job_dirs`] / [`config_dir_for_job`]: which config folder holds a job (the current copy first), to
//!   run `claude attach|rm` there with `CLAUDE_CONFIG_DIR`.
//! - [`job_transcript`], [`project_folder`], [`Successors`]: a job's transcript path, across a git
//!   worktree and `/clear`. (`TranscriptReader` in Swift; the transcript module reads the file itself.)
//! - [`command_job`]: template "exec" jobs, the command shortened and where its output goes.
//! - [`launch_flags`], [`resume_arguments`]: the pure parts of moving a session to another account.
//!
//! # Rules carried over from the Swift app
//!
//! - One row per conversation: when a session moved to another account its stopped copy stays in the
//!   folder it left. The current copy is the one not stopped, then the latest `updatedAt`; never
//!   `createdAt`, since waking an old copy keeps its first creation date.
//! - The live status (`sessions/<pid>.json`) wins over `state.json`: a turn stopped with Esc leaves
//!   `state: "working"` until the next turn.
//! - `template: "exec"` jobs run a shell command: no transcript, no replies ([`Agent::command`]).
//! - `entrypoint: "sdk-cli"` sessions are `claude -p`: headless ([`Agent::headless`]).
//! - A terminal session that handed its work to a background job (`parkedJobId`) is shown as the job.

pub mod agent;
pub mod command_job;
pub mod iso;
pub mod jobs;
pub mod lanes;
pub mod monitor;
pub mod successor;

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
pub use successor::{job_transcript, project_folder, Successors};

/// One poll with a fresh [`Monitor`]: every job and live session across the main folder and the lanes.
pub fn list(roots: &Roots, probes: &Probes, now_ms: i64) -> Vec<Agent> {
    Monitor::default().poll(roots, probes, now_ms)
}
