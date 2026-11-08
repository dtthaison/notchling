//! E11: session control. Pure planners for what the Swift app does to a session, a [`Launcher`] that
//! starts the processes, and fakes for tests. Nothing here types into a session (the reply agent's
//! key plans do that) and nothing runs unless a caller hands a plan to [`execute`].
//!
//! # Public API
//!
//! Planners return a [`Plan`] (steps plus the [`Outcome`] a good run has), or a [`PlanError`] when the
//! session cannot be controlled that way. `roots` is where lanes and jobs are read
//! ([`crate::sessions::Roots`]); a job's lane comes from [`crate::sessions::config_dir_for_job`], as
//! `AgentMonitor.configDir(forJob:)` finds it, and the copy a moved session left behind is handled.
//!
//! | Command | Planner |
//! | --- | --- |
//! | New Session (`claude --bg`, optional trust first) | [`plan_new_session`]`(&`[`NewSession`]`) -> Result<Plan, PlanError>` |
//! | Trust Folder (Down, Enter in a hidden pty) | [`plan_trust_folder`]`(folder, config_dir) -> PtyScript` |
//! | Stop (`claude stop`) | [`plan_stop`]`(&Agent, &Roots) -> Result<Plan, PlanError>` |
//! | Remove (`claude rm` on every copy) | [`plan_remove`]`(&Agent, &Roots) -> Result<Plan, PlanError>` |
//! | Move (stop, `--bg --resume`) | [`plan_move`]`(&Agent, to: Option<&str>, &Roots) -> Result<Plan, PlanError>` |
//! | Move All | [`MoveAllPlan::make`] then [`plan_move_all_jobs`]`(&MoveAllPlan, &Roots) -> Vec<JobMove>` |
//! | Takeover | [`takeover::evaluate`], [`takeover::state`], [`takeover::spawn`], [`offers_takeover`], [`prompt_for`] |
//! | Slash commands | [`slash::all`], [`slash::matches`], [`slash::opens_menu`], [`slash::route`] |
//! | Resume after wake | [`WakeResumer`]`::check(..) -> Vec<Nudge>` |
//! | Git status | [`plan_git_status`]`(folder) -> Vec<Command>`, [`git::read`]`(folder, &dyn Launcher) -> Option<GitInfo>` |
//!
//! Running a plan: [`execute`]`(&Plan, &dyn Launcher, &dyn Fn(u64)) -> Result<Report, ControlError>`.
//! The last argument sleeps for the given milliseconds (after `claude stop` the host needs
//! [`session::STOP_SETTLE_MS`] to let go of the transcript). A `claude --bg` refused for an untrusted
//! folder comes back as [`ControlError::Untrusted`]; plan again with `trust: true`.
//!
//! # Rules
//!
//! - argv[0] is a program *name* (`claude`, `git`); [`RealLauncher`] finds the file. `NO_COLOR` is
//!   never passed on. `env` entries set (`Some`) or remove (`None`) a variable: the main login
//!   removes `CLAUDE_CONFIG_DIR` so it cannot inherit a lane's.
//! - [`Launcher::run_pty`] is `Unsupported` on [`RealLauncher`]: the pty executor lives with the
//!   reply code. A [`PtyScript`] is only data.
//! - Moving needs the transcript (the conversation id is its file stem). The job's `respawnFlags`
//!   are read from the folder holding its current copy and passed along only when the target folder
//!   has no copy of the job to continue ([`crate::sessions::resume_arguments`]).

pub mod git;
pub mod launcher;
pub mod move_all;
pub mod plan;
pub mod run;
pub mod session;
pub mod slash;
pub mod takeover;
pub mod trust;
pub mod wake;

pub use git::{plan_git_status, GitInfo};
pub use launcher::{Call, FakeLauncher, LaunchError, LaunchOutput, Launcher, RealLauncher};
pub use move_all::{plan_move_all_jobs, JobMove, MoveAllPlan, MoveJob};
pub use plan::{
    Command, Expect, KeyStep, Outcome, Plan, PlanError, PtyScript, Quiet, RunStep, Step,
    CONFIG_DIR_VAR,
};
pub use run::{execute, job_id_in_launch_output, ControlError, Report};
pub use session::{plan_move, plan_new_session, plan_remove, plan_stop, NewSession};
pub use slash::{Route, SlashCommand};
pub use takeover::{offers_takeover, prompt_for, TakeoverState};
pub use trust::plan_trust_folder;
pub use wake::{Nudge, WakeResumer};

#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod session_tests;
