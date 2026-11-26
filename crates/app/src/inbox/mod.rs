pub mod banners;
#[cfg(target_os = "macos")]
pub mod build;
#[cfg(target_os = "macos")]
mod change;
pub mod consent_card;
pub mod consent_family;
mod consent_lines;
#[cfg(target_os = "macos")]
pub mod consent_view;
pub mod driver;
pub mod feed;
pub mod model;
pub mod renders;
pub mod renders_b2;
pub mod renders_p8;
#[cfg(target_os = "macos")]
mod rows;
#[cfg(target_os = "macos")]
mod run;
#[cfg(target_os = "macos")]
mod sections;
#[cfg(any(test, target_os = "macos"))]
mod tab_reveal;
#[cfg(test)]
mod tab_reveal_tests;
pub mod tray_rows;
pub mod triage;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(target_os = "macos")]
mod view_family;
pub mod warning;

pub const WIDTH: f32 = 360.0;
pub const MAX_HEIGHT: f32 = 860.0;
pub const BLOCKING_MIN: f32 = 300.0;
pub const BLOCKING_RESERVE: f32 = 230.0;
pub const OUTSIDE: f32 = 56.0;
pub const PANEL_MIN: f32 = 320.0;
pub const PANEL_CHROME: f32 = 100.0;
pub const TABS_SHARE: f32 = 120.0;

pub(crate) mod family_fixtures;
#[cfg(test)]
mod tray_rows_tests;
#[cfg(test)]
mod triage_tests;
#[cfg(test)]
mod versions_drift_rows_tests;
#[cfg(test)]
mod versions_rows_tests;
#[cfg(all(test, target_os = "macos"))]
mod view_tests;
