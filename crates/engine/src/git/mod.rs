pub mod branch;
pub mod branch_file;
pub mod branch_parse;
pub mod guard;
pub mod porcelain;
pub mod read;
pub mod sandbox;

pub use branch::{read_branch_stat, read_branches};
pub use branch_file::read_branch_diff;
pub use branch_parse::{BranchFile, BranchStat, Branches, MAX_BRANCH_FILES};
pub use porcelain::{Change, Status, MAX_FILES};
pub use read::{read_diff, read_status, Diff, Failure, MAX_DIFF_BYTES, MAX_DIFF_LINES};
pub use sandbox::SandboxedLauncher;

#[cfg(test)]
mod branch_parse_tests;
#[cfg(all(test, unix))]
mod branch_real_tests;
#[cfg(test)]
mod branch_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod porcelain_tests;
#[cfg(all(test, unix))]
mod read_hardening_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod sandbox_tests;
