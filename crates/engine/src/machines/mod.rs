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
pub mod steps_machine;
pub mod steps_secret;
mod steps_xcode;
pub mod token_creation;
pub mod token_scope;
mod token_scope_net;
pub mod usage;

#[cfg(test)]
pub mod fakes;
#[cfg(test)]
mod tests_machine_steps;
#[cfg(test)]
mod tests_machine_tools;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_runner;
#[cfg(test)]
mod tests_secret_steps;
#[cfg(test)]
mod tests_secrets;
#[cfg(test)]
mod tests_shell;
#[cfg(test)]
mod tests_token_scope;
#[cfg(test)]
mod tests_token_scope_gh;
#[cfg(test)]
mod tests_token_scope_view;
