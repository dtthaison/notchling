pub mod core;
pub mod id;
pub mod jobs;
pub mod join;
pub mod members;
pub mod persist;
pub mod policy;
pub mod state;
pub mod versions;
pub mod wire;

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
mod id_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod versions_tests;
#[cfg(test)]
mod wire_tests;
