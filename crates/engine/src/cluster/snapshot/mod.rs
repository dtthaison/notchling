pub mod manifest;
pub mod repo_state;
pub mod stand_in;

pub use manifest::{dir_digest, staged_path, Manifest, Part, PartKind};
pub use repo_state::{
    filter_probe, movable, parse, read_plan, without_filters, GitCall, GitOutput, Movable,
    RepoFacts,
};
pub use stand_in::{excluded, SessionEntry, SetPlan, Trigger, EXCLUDED_DIRS, MAX_FILE};

#[cfg(test)]
mod manifest_tests;
#[cfg(test)]
mod repo_state_index_tests;
#[cfg(test)]
mod repo_state_tests;
