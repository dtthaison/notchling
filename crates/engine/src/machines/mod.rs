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

pub mod engine;
pub mod engine_check;
pub mod engine_fix;
pub mod engine_plan;
pub mod forms;
pub mod github;
pub mod masks;
pub mod offers;
pub mod process;
pub mod registry;
pub mod roots;
pub mod runner;
pub mod shell;
pub mod step;
pub mod usage;

#[cfg(test)]
mod tests_engine;
#[cfg(test)]
mod tests_engine_fix;
#[cfg(test)]
mod tests_engine_offers;
#[cfg(test)]
pub mod tests_engine_world;
#[cfg(test)]
mod tests_forms;
