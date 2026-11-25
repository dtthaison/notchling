pub mod fake;
pub mod fetch;
pub mod manifest;
pub mod net;

pub use fetch::{fetch_file, file_matches, Fetch, Head, Progress, Receiver, CANCELLED};
pub use manifest::{manifest, Manifest, ModelEntry, ModelFile};

#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod manifest_tests;
