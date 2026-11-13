pub mod guard;
pub mod porcelain;
pub mod read;

pub use porcelain::{Change, Status, MAX_FILES};
pub use read::{read_diff, read_status, Diff, Failure, MAX_DIFF_BYTES, MAX_DIFF_LINES};

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod porcelain_tests;
#[cfg(test)]
mod read_tests;
