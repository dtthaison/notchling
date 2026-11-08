//! Machines (E10; Swift `Machines/`): the Macs and servers that run sd's overnight worker, set up
//! and watched from one window.
//!
//! - [`registry`]: `machines.json` v2 with jobs. A v1 file is backed up byte for byte before the
//!   first write; a file that can't be read is never written over.
//! - [`step`]: every setup step is a Check (a read-only probe) and a Fix. The engine runs the checks
//!   (one ssh per machine per refresh) and a Fix only through plan, confirm, run, so nothing
//!   changes a machine unless its exact commands were shown first.
//! - [`runner`]: every process goes through [`runner::Runner`]; tests use
//!   [`runner::ScriptedRunner`] and never start ssh, `gh` or `sudo`.
//! - Secrets travel only on a process's stdin or in GitHub's Authorization header, and every text a
//!   machine sent is masked ([`masks`]) before anyone sees it.

pub mod github;
pub mod masks;
pub mod process;
pub mod registry;
pub mod runner;
pub mod shell;
pub mod step;

// Jobs: worker status, the poller and health, the morning list, merge plans and morning actions.
pub mod actions;
mod actions_util;
pub mod merge;
pub mod merge_ops;
pub mod morning;
pub mod poller;
pub mod status;

#[cfg(test)]
pub mod fakes_jobs;
#[cfg(test)]
mod tests_actions;
#[cfg(test)]
mod tests_actions_files;
#[cfg(test)]
mod tests_actions_gh;
#[cfg(test)]
mod tests_actions_read;
#[cfg(test)]
mod tests_merge;
#[cfg(test)]
mod tests_merge_b;
#[cfg(test)]
mod tests_merge_c;
#[cfg(test)]
mod tests_merge_git;
#[cfg(test)]
mod tests_merge_git_b;
#[cfg(test)]
mod tests_morning;
#[cfg(test)]
mod tests_poller;
#[cfg(test)]
mod tests_status;
