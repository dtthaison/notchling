pub mod guard;
pub mod porcelain;
pub mod read;
pub mod sandbox;

pub use porcelain::{Change, Status, MAX_FILES};
pub use read::{read_diff, read_status, Diff, Failure, MAX_DIFF_BYTES, MAX_DIFF_LINES};
pub use sandbox::SandboxedLauncher;

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
