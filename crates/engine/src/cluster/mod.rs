pub mod blob;
pub mod core;
pub mod failover;
pub mod fence;
pub mod grant_files;
mod grant_files_sys;
pub mod grants;
pub mod id;
pub mod jobs;
pub mod join;
pub mod members;
pub mod outward;
pub mod persist;
pub mod policy;
pub mod shell_job;
pub mod snapshot;
pub mod state;
pub mod versions;
pub mod wire;
pub mod workspace;

#[cfg(test)]
pub mod sim;
#[cfg(test)]
pub mod testkit;

pub use core::{Effect, Input, LocalCmd, NodeCore};
pub use id::{fingerprint, qualify, tool_of, NodeId, Qualified, Role, Tool};
pub use jobs::{Job, JobSpec, JobState};
pub use members::{Desired, Member, MemberState, Term};
pub use state::{role_of, ClusterState, Command, JournalEntry};
pub use versions::{compat, Compat, Purpose, Versions};

#[cfg(test)]
mod blob_tests;
#[cfg(test)]
mod failover_tests;
#[cfg(test)]
mod fence_tests;
#[cfg(test)]
mod grant_files_hostile_tests;
#[cfg(test)]
mod grant_files_tests;
#[cfg(test)]
mod grants_tests;
#[cfg(test)]
mod id_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod outward_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod shell_job_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod versions_tests;
#[cfg(test)]
mod wire_tests;
#[cfg(test)]
mod workspace_tests;
