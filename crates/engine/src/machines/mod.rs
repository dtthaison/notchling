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

pub mod actions;
mod actions_util;
pub mod catalog;
pub mod engine;
pub mod engine_check;
pub mod engine_fix;
pub mod engine_plan;
pub mod forms;
pub mod github;
pub mod mac_facts;
pub mod masks;
pub mod merge;
pub mod merge_ops;
pub mod morning;
pub mod offers;
pub mod poller;
pub mod process;
pub mod registry;
pub mod repo_common;
pub mod roots;
pub mod runner;
pub mod service_files;
pub mod shell;
pub mod status;
pub mod step;
pub mod steps_machine;
pub mod steps_push;
pub mod steps_replace;
pub mod steps_repo;
pub mod steps_sd;
pub mod steps_sd_check;
pub mod steps_sd_service;
pub mod steps_secret;
mod steps_xcode;
pub mod token_creation;
pub mod token_scope;
mod token_scope_net;
pub mod usage;
pub mod worker_json;

#[cfg(test)]
pub mod fakes;
#[cfg(test)]
pub mod fakes_b;
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
mod tests_engine;
#[cfg(test)]
mod tests_engine_fix;
#[cfg(test)]
mod tests_engine_offers;
#[cfg(test)]
pub mod tests_engine_world;
#[cfg(test)]
mod tests_engine_writes;
#[cfg(test)]
mod tests_forms;
#[cfg(test)]
mod tests_machine_steps;
#[cfg(test)]
mod tests_machine_tools;
#[cfg(test)]
mod tests_merge;
#[cfg(test)]
mod tests_merge_b;
#[cfg(test)]
mod tests_merge_c;
#[cfg(all(test, unix))]
mod tests_merge_git;
#[cfg(all(test, unix))]
mod tests_merge_git_b;
#[cfg(test)]
mod tests_morning;
#[cfg(test)]
mod tests_poller;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_repo;
#[cfg(test)]
mod tests_repo_push;
#[cfg(test)]
mod tests_repo_replace;
#[cfg(test)]
mod tests_repo_replace_b;
#[cfg(all(test, unix))]
mod tests_repo_shell;
#[cfg(all(test, unix))]
mod tests_runner;
#[cfg(test)]
mod tests_safety;
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
#[cfg(test)]
mod tests_secret_steps;
#[cfg(test)]
mod tests_secrets;
#[cfg(test)]
mod tests_shell;
#[cfg(test)]
mod tests_status;
#[cfg(test)]
mod tests_token_scope;
#[cfg(test)]
mod tests_token_scope_gh;
#[cfg(test)]
mod tests_token_scope_view;
