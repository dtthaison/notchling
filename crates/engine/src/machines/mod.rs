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
pub mod mac_facts;
pub mod masks;
pub mod process;
pub mod registry;
pub mod repo_common;
pub mod runner;
pub mod service_files;
pub mod shell;
pub mod step;
pub mod steps_push;
pub mod steps_replace;
pub mod steps_repo;
pub mod steps_sd;
pub mod steps_sd_check;
pub mod steps_sd_service;
pub mod worker_json;

#[cfg(test)]
pub mod fakes_b;
#[cfg(test)]
mod tests_repo;
#[cfg(test)]
mod tests_repo_push;
#[cfg(test)]
mod tests_repo_replace;
#[cfg(test)]
mod tests_repo_replace_b;
#[cfg(test)]
mod tests_repo_shell;
#[cfg(test)]
mod tests_safety_b;
#[cfg(test)]
mod tests_sd;
#[cfg(test)]
mod tests_sd_check;
#[cfg(test)]
mod tests_sd_service;
#[cfg(test)]
mod tests_sd_service_b;
#[cfg(test)]
mod tests_sd_worker;
