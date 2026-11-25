#[cfg(target_os = "macos")]
pub mod build;
pub mod consent_card;
pub mod consent_family;
mod consent_lines;
#[cfg(target_os = "macos")]
pub mod consent_view;
pub mod feed;
pub mod model;
pub mod renders;
pub mod renders_p8;
#[cfg(target_os = "macos")]
mod run;
pub mod system;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
mod view_family;

pub const WIDTH: f32 = 360.0;
pub const MAX_HEIGHT: f32 = 520.0;

pub(crate) mod family_fixtures;
#[cfg(test)]
mod versions_drift_rows_tests;
#[cfg(test)]
mod versions_rows_tests;
